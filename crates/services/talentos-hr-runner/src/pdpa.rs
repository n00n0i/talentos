//! PDPA retention + deletion/anonymisation (DSAR) per tenant.
//!
//! Policy defaults (builder's call, handoff §8.2): payroll/GL-related PII is
//! retained 7 years (legal obligation — never purged); operational data
//! (attendance, leave, OT, claims, documents) is purged `ops_retention_months`
//! after termination. `purge_employee_pii` anonymises identity fields in place;
//! `run_retention_purge` applies the same logic to every employee whose
//! retention window has expired. Everything is tenant-scoped via
//! `begin_tenant` + explicit `tenant_id` predicates and audited in the same
//! transaction.

use chrono::NaiveDate;
use sqlx::Row;
use talentos_db::TenantPool;
use talentos_kernel::TenantId;
use uuid::Uuid;

/// Legal minimum: payroll/GL-related PII is retained this many years and is
/// never purged by the retention job.
pub const DEFAULT_PAYROLL_RETENTION_YEARS: i32 = 7;

/// Operational data (attendance, leave, OT, claims, documents) is purged this
/// many months after termination.
pub const DEFAULT_OPS_RETENTION_MONTHS: i32 = 12;

/// The tenant's retention policy. Rows are created lazily with the defaults.
#[derive(Debug, Clone, Copy)]
pub struct PdpaPolicy {
    pub payroll_retention_years: i32,
    pub ops_retention_months: i32,
}

impl Default for PdpaPolicy {
    fn default() -> Self {
        Self {
            payroll_retention_years: DEFAULT_PAYROLL_RETENTION_YEARS,
            ops_retention_months: DEFAULT_OPS_RETENTION_MONTHS,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum PdpaError {
    #[error("employee not found for this tenant")]
    EmployeeNotFound,
    #[error("database error: {0}")]
    Db(#[from] sqlx::Error),
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct PurgeResult {
    pub employee_id: Uuid,
    /// Identity fields were actually cleared (false when already NULL).
    pub pii_anonymised: bool,
    /// Candidate contact rows whose email/phone were nulled.
    pub candidate_rows_anonymised: i64,
    /// Operational rows deleted by the retention cutoff.
    pub operational_rows_removed: i64,
}

/// The tenant's retention policy; the row is created with the legal defaults
/// on first read.
pub async fn get_policy(pool: &TenantPool, tenant_id: TenantId) -> Result<PdpaPolicy, PdpaError> {
    let mut tx = pool.begin_tenant(tenant_id).await?;
    let row = sqlx::query(
        r#"INSERT INTO tenant_pdpa_policy (tenant_id) VALUES ($1)
            ON CONFLICT (tenant_id) DO UPDATE SET tenant_id = EXCLUDED.tenant_id
            RETURNING payroll_retention_years, ops_retention_months"#,
    )
    .bind(tenant_id.0)
    .fetch_one(&mut *tx)
    .await?;
    let policy = PdpaPolicy {
        payroll_retention_years: row.get("payroll_retention_years"),
        ops_retention_months: row.get("ops_retention_months"),
    };
    tx.commit().await?;
    Ok(policy)
}

/// Operational-data cutoff: `termination_date + ops_retention_months` (30-day
/// months; fine at retention-granularity). For active employees there is no
/// termination date, so nothing is purged — the job only touches terminated
/// employees.
fn ops_cutoff(termination_date: Option<NaiveDate>, ops_retention_months: i32) -> Option<NaiveDate> {
    let due = termination_date?;
    Some(due + chrono::Duration::days(ops_retention_months as i64 * 30))
}

/// Anonymise one employee's identity PII (national_id, bank account, SSO
/// number → NULL; candidate email/phone → NULL), purge operational data past
/// the tenant's retention window, and audit it. Payroll/GL rows are
/// deliberately untouched — legal obligation. Idempotent: re-running clears
/// nothing and reports zeroes.
pub async fn purge_employee_pii(
    pool: &TenantPool,
    tenant_id: TenantId,
    employee_id: Uuid,
    actor_id: Uuid,
    actor_type: &str,
) -> Result<PurgeResult, PdpaError> {
    let policy = get_policy(pool, tenant_id).await?;
    let mut tx = pool.begin_tenant(tenant_id).await?;

    let employee = sqlx::query(
        "SELECT termination_date FROM employees
          WHERE tenant_id = $1 AND id = $2",
    )
    .bind(tenant_id.0)
    .bind(employee_id)
    .fetch_optional(&mut *tx)
    .await?;
    let Some(employee) = employee else {
        tx.rollback().await?;
        return Err(PdpaError::EmployeeNotFound);
    };

    let pii_anonymised = sqlx::query(
        "UPDATE employees
            SET national_id = NULL, bank_account_number = NULL, sso_number = NULL,
                pii_purged_at = COALESCE(pii_purged_at, now()), updated_at = now()
          WHERE tenant_id = $1 AND id = $2
            AND (national_id IS NOT NULL
                 OR bank_account_number IS NOT NULL
                 OR sso_number IS NOT NULL
                 OR pii_purged_at IS NULL)",
    )
    .bind(tenant_id.0)
    .bind(employee_id)
    .execute(&mut *tx)
    .await?
    .rows_affected()
        > 0;

    let candidate_rows_anonymised: i64 = sqlx::query(
        "UPDATE candidates SET email = NULL, phone = NULL
          WHERE tenant_id = $1 AND created_by = $2
            AND (email IS NOT NULL OR phone IS NOT NULL)",
    )
    .bind(tenant_id.0)
    .bind(employee_id)
    .execute(&mut *tx)
    .await?
    .rows_affected() as i64;

    let mut operational_rows_removed = 0i64;
    let cutoff = ops_cutoff(
        employee.get::<Option<NaiveDate>, _>("termination_date"),
        policy.ops_retention_months,
    );
    if let Some(cutoff) = cutoff {
        let pairs = [
            (
                "SELECT count(*) FROM attendance_days
                  WHERE tenant_id = $1 AND employee_id = $2 AND work_date < $3",
                "DELETE FROM attendance_days
                  WHERE tenant_id = $1 AND employee_id = $2 AND work_date < $3",
            ),
            (
                "SELECT count(*) FROM attendance_events
                  WHERE tenant_id = $1 AND employee_id = $2 AND work_date < $3",
                "DELETE FROM attendance_events
                  WHERE tenant_id = $1 AND employee_id = $2 AND work_date < $3",
            ),
            (
                "SELECT count(*) FROM leave_requests
                  WHERE tenant_id = $1 AND employee_id = $2 AND end_date < $3",
                "DELETE FROM leave_requests
                  WHERE tenant_id = $1 AND employee_id = $2 AND end_date < $3",
            ),
            (
                "SELECT count(*) FROM ot_requests
                  WHERE tenant_id = $1 AND employee_id = $2 AND work_date < $3",
                "DELETE FROM ot_requests
                  WHERE tenant_id = $1 AND employee_id = $2 AND work_date < $3",
            ),
            (
                "SELECT count(*) FROM expense_claims
                  WHERE tenant_id = $1 AND employee_id = $2 AND expense_date < $3",
                "DELETE FROM expense_claims
                  WHERE tenant_id = $1 AND employee_id = $2 AND expense_date < $3",
            ),
            (
                "SELECT count(*) FROM employee_documents
                  WHERE tenant_id = $1 AND employee_id = $2
                    AND (expiry_date IS NULL OR expiry_date < $3)",
                "DELETE FROM employee_documents
                  WHERE tenant_id = $1 AND employee_id = $2
                    AND (expiry_date IS NULL OR expiry_date < $3)",
            ),
        ];
        for (count_sql, delete_sql) in pairs {
            let count: i64 = sqlx::query_scalar(count_sql)
                .bind(tenant_id.0)
                .bind(employee_id)
                .bind(cutoff)
                .fetch_one(&mut *tx)
                .await?;
            if count == 0 {
                continue;
            }
            sqlx::query(delete_sql)
                .bind(tenant_id.0)
                .bind(employee_id)
                .bind(cutoff)
                .execute(&mut *tx)
                .await?;
            operational_rows_removed += count;
        }
    }

    sqlx::query(
        r#"INSERT INTO audit_log
             (tenant_id, actor_type, actor_id, action, resource_type, resource_id, after_state)
           VALUES ($1, $2, $3, 'privacy.purge', 'employee', $4, $5)"#,
    )
    .bind(tenant_id.0)
    .bind(actor_type)
    .bind(actor_id.to_string())
    .bind(employee_id.to_string())
    .bind(serde_json::json!({
        "pii_anonymised": pii_anonymised,
        "candidate_rows_anonymised": candidate_rows_anonymised,
        "operational_rows_removed": operational_rows_removed,
        "payroll_retention_years": policy.payroll_retention_years,
        "payroll_gl_retained": true,
    }))
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;

    Ok(PurgeResult {
        employee_id,
        pii_anonymised,
        candidate_rows_anonymised,
        operational_rows_removed,
    })
}

/// Every terminated employee whose operational-data window has fully elapsed
/// and who has not been purged yet. Purges each one and returns the results
/// so the gateway can report what happened.
pub async fn run_retention_purge(
    pool: &TenantPool,
    tenant_id: TenantId,
    actor_id: Uuid,
    actor_type: &str,
) -> Result<Vec<PurgeResult>, PdpaError> {
    let policy = get_policy(pool, tenant_id).await?;

    let due: Vec<Uuid> = {
        let mut tx = pool.begin_tenant(tenant_id).await?;
        let ids = sqlx::query_scalar::<_, Uuid>(
            r#"SELECT id FROM employees
                WHERE tenant_id = $1
                  AND status = 'terminated'
                  AND termination_date IS NOT NULL
                  AND termination_date <=
                        (now()::date - ($2::int || ' months')::interval)
                  AND pii_purged_at IS NULL"#,
        )
        .bind(tenant_id.0)
        .bind(policy.ops_retention_months.to_string())
        .fetch_all(&mut *tx)
        .await?;
        let _ = tx.commit().await;
        ids
    };

    let mut results = Vec::with_capacity(due.len());
    for employee_id in due {
        let result = purge_employee_pii(pool, tenant_id, employee_id, actor_id, actor_type).await?;
        results.push(result);
    }
    Ok(results)
}

/// The tenant's policy value pair, for the settings UI.
pub fn policy_defaults() -> (i32, i32) {
    (
        DEFAULT_PAYROLL_RETENTION_YEARS,
        DEFAULT_OPS_RETENTION_MONTHS,
    )
}
