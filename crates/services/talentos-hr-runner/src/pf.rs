//! Provident fund contribution rates, effective-dated per employee.
//!
//! The rate applied to a payroll period is the latest `effective_from` on or
//! before the period end; an employee with no row falls back to the statutory
//! default 5%.

use chrono::NaiveDate;
use rust_decimal::Decimal;
use serde::Serialize;
use sqlx::{PgConnection, Row};
use talentos_db::TenantPool;
use talentos_kernel::TenantId;
use uuid::Uuid;

/// Statutory default PF contribution rate (5%) when no history row matches.
pub const DEFAULT_PF_RATE: Decimal = rust_decimal_macros::dec!(0.05);

/// Upper bound for a PF rate (15%).
pub const MAX_PF_RATE: Decimal = rust_decimal_macros::dec!(0.15);

/// One effective-dated rate row.
#[derive(Debug, Clone, Serialize)]
pub struct PfRate {
    pub id: i64,
    pub effective_from: NaiveDate,
    pub rate: Decimal,
}

/// PF service errors.
#[derive(Debug, thiserror::Error)]
pub enum PfError {
    #[error("employee not found in this tenant")]
    EmployeeNotFound,
    #[error("rate must be between 0 and 0.15")]
    RateOutOfRange,
    #[error("payroll run is not approved")]
    NotApproved,
    #[error("payroll run not found in this tenant")]
    RunNotFound,
    #[error("database error: {0}")]
    Db(#[from] sqlx::Error),
}

/// One member row in a PF remittance payload.
#[derive(Debug, Clone, Serialize)]
pub struct PfRemittanceMember {
    pub employee_code: String,
    pub name: String,
    pub employee_amount: Decimal,
    pub employer_amount: Decimal,
    pub total: Decimal,
}

/// Provider-agnostic remittance payload for an approved run.
///
/// Provider-specific field mapping (fund codes, member IDs, etc.) is a future
/// per-provider adapter; this is the shared shape every adapter consumes.
#[derive(Debug, Clone, Serialize)]
pub struct PfRemittancePayload {
    pub payroll_run_id: Uuid,
    pub period_start: NaiveDate,
    pub period_end: NaiveDate,
    pub members: Vec<PfRemittanceMember>,
    pub total_employee: Decimal,
    pub total_employer: Decimal,
    pub total: Decimal,
}

/// Return the PF rate effective for `employee_id` on `as_of`: the latest row
/// with `effective_from <= as_of`, else the default 5%.
pub async fn rate_for(
    pool: &TenantPool,
    tenant_id: TenantId,
    employee_id: Uuid,
    as_of: NaiveDate,
) -> Result<Decimal, PfError> {
    let mut tx = pool.begin_tenant(tenant_id).await?;
    let rate: Option<Decimal> = sqlx::query_scalar(
        r#"SELECT rate FROM pf_rate_history
            WHERE tenant_id = current_tenant_id()
              AND employee_id = $1
              AND effective_from <= $2
            ORDER BY effective_from DESC
            LIMIT 1"#,
    )
    .bind(employee_id)
    .bind(as_of)
    .fetch_optional(&mut *tx)
    .await?;
    let _ = tx.rollback().await;
    Ok(rate.unwrap_or(DEFAULT_PF_RATE))
}

/// Return every rate row for an employee, newest first.
pub async fn list_rates(
    pool: &TenantPool,
    tenant_id: TenantId,
    employee_id: Uuid,
) -> Result<Vec<PfRate>, PfError> {
    let mut tx = pool.begin_tenant(tenant_id).await?;
    let rows = sqlx::query(
        r#"SELECT id, effective_from, rate FROM pf_rate_history
            WHERE tenant_id = current_tenant_id() AND employee_id = $1
            ORDER BY effective_from DESC"#,
    )
    .bind(employee_id)
    .fetch_all(&mut *tx)
    .await?;
    let _ = tx.rollback().await;
    rows.iter()
        .map(|row| {
            Ok(PfRate {
                id: row.try_get("id")?,
                effective_from: row.try_get("effective_from")?,
                rate: row.try_get("rate")?,
            })
        })
        .collect()
}

/// Upsert the rate for `effective_from`, replacing any row on that date.
pub async fn upsert_rate(
    pool: &TenantPool,
    tenant_id: TenantId,
    employee_id: Uuid,
    effective_from: NaiveDate,
    rate: Decimal,
    created_by: Option<Uuid>,
) -> Result<(), PfError> {
    if rate < Decimal::ZERO || rate > MAX_PF_RATE {
        return Err(PfError::RateOutOfRange);
    }

    let mut tx = pool.begin_tenant(tenant_id).await?;
    let exists: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM employees WHERE id = $1 AND tenant_id = current_tenant_id())",
    )
    .bind(employee_id)
    .fetch_one(&mut *tx)
    .await?;
    if !exists {
        return Err(PfError::EmployeeNotFound);
    }

    sqlx::query(
        r#"INSERT INTO pf_rate_history (tenant_id, employee_id, effective_from, rate, created_by)
            VALUES (current_tenant_id(), $1, $2, $3, $4)
            ON CONFLICT (tenant_id, employee_id, effective_from) DO UPDATE SET
                rate = EXCLUDED.rate,
                created_by = EXCLUDED.created_by,
                created_at = now()"#,
    )
    .bind(employee_id)
    .bind(effective_from)
    .bind(rate)
    .bind(created_by)
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;
    Ok(())
}

/// Build the PF remittance payload for an approved payroll run.
///
/// Only payslips with a non-zero employee or employer PF contribution are
/// included. A non-approved run is `NotApproved`; a run invisible to the tenant
/// (or missing) is `RunNotFound`. Caller must have scoped the connection with
/// `current_tenant_id()` (RLS enforces tenant isolation regardless).
pub async fn build_remittance_payload(
    conn: &mut PgConnection,
    run_id: Uuid,
) -> Result<PfRemittancePayload, PfError> {
    let run = sqlx::query(
        "SELECT status, period_start, period_end FROM payroll_runs \
         WHERE id = $1 AND tenant_id = current_tenant_id()",
    )
    .bind(run_id)
    .fetch_optional(&mut *conn)
    .await?;

    let (period_start, period_end) = match run {
        Some(row) => {
            let status: String = row.try_get("status")?;
            if status != "approved" {
                return Err(PfError::NotApproved);
            }
            (row.try_get("period_start")?, row.try_get("period_end")?)
        }
        None => return Err(PfError::RunNotFound),
    };

    let rows = sqlx::query(
        "SELECT e.employee_code, \
                e.first_name_th || ' ' || e.last_name_th AS name, \
                p.pf_employee, p.pf_employer \
         FROM payslips p \
         JOIN employees e ON e.id = p.employee_id AND e.tenant_id = p.tenant_id \
         WHERE p.payroll_run_id = $1 \
           AND p.tenant_id = current_tenant_id() \
           AND (p.pf_employee > 0 OR p.pf_employer > 0) \
         ORDER BY e.employee_code",
    )
    .bind(run_id)
    .fetch_all(&mut *conn)
    .await?;

    let mut total_employee = Decimal::ZERO;
    let mut total_employer = Decimal::ZERO;
    let mut members = Vec::with_capacity(rows.len());
    for row in &rows {
        let employee: Decimal = row.try_get("pf_employee")?;
        let employer: Decimal = row.try_get("pf_employer")?;
        total_employee += employee;
        total_employer += employer;
        members.push(PfRemittanceMember {
            employee_code: row.try_get("employee_code")?,
            name: row.try_get("name")?,
            employee_amount: employee,
            employer_amount: employer,
            total: employee + employer,
        });
    }

    Ok(PfRemittancePayload {
        payroll_run_id: run_id,
        period_start,
        period_end,
        members,
        total_employee,
        total_employer,
        total: total_employee + total_employer,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_and_bounds() {
        assert_eq!(DEFAULT_PF_RATE, Decimal::new(5, 2));
        assert_eq!(MAX_PF_RATE, Decimal::new(15, 2));
    }
}
