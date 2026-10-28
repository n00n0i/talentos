//! Final settlement calculator and draft storage.
//!
//! Computes last-month proration, unused leave pay, notice pay and statutory
//! severance, then stores a draft snapshot. Tax withholding and payroll/GL
//! integration are intentionally out of scope.

use chrono::{Datelike, NaiveDate};
use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::Row;
use talentos_compliance::{calculate_severance, SeveranceTier};
use talentos_db::TenantPool;
use talentos_kernel::TenantId;
use uuid::Uuid;

const MAX_DAYS_PER_MONTH: i32 = 30;

#[derive(Debug, thiserror::Error)]
pub enum SettlementError {
    #[error("employee not found for this tenant")]
    EmployeeNotFound,
    #[error("termination_date must be on or after hire_date")]
    InvalidTerminationDate,
    #[error("{0} must be >= 0")]
    NegativeDays(&'static str),
    #[error("base_salary must be >= 0")]
    NegativeSalary,
    #[error("compliance error: {0}")]
    Compliance(#[from] talentos_compliance::ComplianceError),
    #[error("database error: {0}")]
    Db(#[from] sqlx::Error),
}

#[derive(Debug, Deserialize)]
pub struct SettlementInput {
    pub termination_date: NaiveDate,
    pub unused_leave_days: i32,
    pub notice_days: i32,
    pub include_severance: bool,
}

#[derive(Debug, Serialize)]
pub struct SettlementView {
    pub id: Uuid,
    pub employee_id: Uuid,
    pub employee_code: String,
    pub employee_name: String,
    pub termination_date: NaiveDate,
    pub unused_leave_days: i32,
    pub notice_days: i32,
    pub include_severance: bool,
    pub years_of_service: f64,
    pub last_month_pay: Decimal,
    pub leave_pay: Decimal,
    pub notice_pay: Decimal,
    pub severance_pay: Decimal,
    pub total_gross: Decimal,
    pub breakdown: serde_json::Value,
    pub status: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

/// Compute a final settlement for an employee and store it as a draft.
pub async fn compute_and_store(
    pool: &TenantPool,
    tenant_id: TenantId,
    created_by: Uuid,
    employee_id: Uuid,
    input: SettlementInput,
) -> Result<SettlementView, SettlementError> {
    validate(&input)?;

    let mut tx = pool.begin_tenant(tenant_id).await?;

    let row = sqlx::query(
        r#"SELECT employee_code, first_name_th, last_name_th, base_salary, hire_date
           FROM employees
           WHERE id = $1 AND tenant_id = $2
           LIMIT 1"#,
    )
    .bind(employee_id)
    .bind(tenant_id.0)
    .fetch_optional(&mut *tx)
    .await?;

    let Some(row) = row else {
        let _ = tx.rollback().await;
        return Err(SettlementError::EmployeeNotFound);
    };

    let employee_code: String = row.try_get("employee_code")?;
    let first_name_th: String = row.try_get("first_name_th")?;
    let last_name_th: String = row.try_get("last_name_th")?;
    let base_salary: Decimal = row.try_get("base_salary")?;
    let hire_date: NaiveDate = row.try_get("hire_date")?;

    if input.termination_date < hire_date {
        let _ = tx.rollback().await;
        return Err(SettlementError::InvalidTerminationDate);
    }
    if base_salary < Decimal::ZERO {
        let _ = tx.rollback().await;
        return Err(SettlementError::NegativeSalary);
    }

    let daily_rate = base_salary / dec!(30);
    let last_month_days = (input.termination_date.day() as i32).min(MAX_DAYS_PER_MONTH);

    let last_month_pay = (daily_rate * Decimal::from(last_month_days)).round_dp(2);
    let leave_pay = (daily_rate * Decimal::from(input.unused_leave_days)).round_dp(2);
    let notice_pay = (daily_rate * Decimal::from(input.notice_days)).round_dp(2);

    // Completed calendar years, not 365-day blocks: an employee reaches the
    // next service tier on the anniversary, not after 365 days.
    let mut completed_years = input.termination_date.year() - hire_date.year();
    if (input.termination_date.month(), input.termination_date.day())
        < (hire_date.month(), hire_date.day())
    {
        completed_years -= 1;
    }
    let years_decimal = Decimal::from(completed_years.max(0));
    let years_of_service = years_decimal.to_f64().unwrap_or(0.0);

    let severance_pay = if input.include_severance {
        calculate_severance(base_salary, years_of_service)?
    } else {
        Decimal::ZERO
    };

    let total_gross = (last_month_pay + leave_pay + notice_pay + severance_pay).round_dp(2);

    let severance_days = severance_days_for(years_of_service);
    let breakdown = json!({
        "daily_rate": daily_rate.round_dp(2),
        "last_month_days": last_month_days,
        "unused_leave_days": input.unused_leave_days,
        "notice_days": input.notice_days,
        "years_of_service": years_of_service,
        "severance_days": severance_days,
        "include_severance": input.include_severance,
        "last_month_pay": last_month_pay,
        "leave_pay": leave_pay,
        "notice_pay": notice_pay,
        "severance_pay": severance_pay,
        "total_gross": total_gross,
    });

    let id = Uuid::new_v4();
    let insert = sqlx::query(
        r#"INSERT INTO final_settlements
             (id, tenant_id, employee_id, termination_date, unused_leave_days, notice_days,
              include_severance, years_of_service, last_month_pay, leave_pay, notice_pay,
              severance_pay, total_gross, breakdown, status, created_by)
           VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,'draft',$15)
           RETURNING id, created_at"#,
    )
    .bind(id)
    .bind(tenant_id.0)
    .bind(employee_id)
    .bind(input.termination_date)
    .bind(input.unused_leave_days)
    .bind(input.notice_days)
    .bind(input.include_severance)
    .bind(years_decimal)
    .bind(last_month_pay)
    .bind(leave_pay)
    .bind(notice_pay)
    .bind(severance_pay)
    .bind(total_gross)
    .bind(breakdown.clone())
    .bind(created_by)
    .fetch_one(&mut *tx)
    .await?;

    tx.commit().await?;

    Ok(SettlementView {
        id: insert.try_get("id")?,
        employee_id,
        employee_code,
        employee_name: format!("{} {}", first_name_th, last_name_th),
        termination_date: input.termination_date,
        unused_leave_days: input.unused_leave_days,
        notice_days: input.notice_days,
        include_severance: input.include_severance,
        years_of_service,
        last_month_pay,
        leave_pay,
        notice_pay,
        severance_pay,
        total_gross,
        breakdown,
        status: "draft".to_string(),
        created_at: insert.try_get("created_at")?,
    })
}

/// List stored settlements for a tenant, optionally filtered to one employee.
pub async fn list_settlements(
    pool: &TenantPool,
    tenant_id: TenantId,
    employee_id: Option<Uuid>,
) -> Result<Vec<SettlementView>, SettlementError> {
    let mut tx = pool.begin_tenant(tenant_id).await?;
    let rows = sqlx::query(
        r#"SELECT s.id, s.employee_id, s.termination_date, s.unused_leave_days, s.notice_days,
                  s.include_severance, s.years_of_service, s.last_month_pay, s.leave_pay,
                  s.notice_pay, s.severance_pay, s.total_gross, s.breakdown, s.status,
                  s.created_at, e.employee_code, e.first_name_th, e.last_name_th
           FROM final_settlements s
           JOIN employees e ON e.id = s.employee_id
           WHERE s.tenant_id = $1
             AND ($2::uuid IS NULL OR s.employee_id = $2)
           ORDER BY s.created_at DESC
           LIMIT 200"#,
    )
    .bind(tenant_id.0)
    .bind(employee_id)
    .fetch_all(&mut *tx)
    .await?;
    tx.commit().await?;

    rows.into_iter()
        .map(row_to_view)
        .collect::<Result<Vec<_>, sqlx::Error>>()
        .map_err(Into::into)
}

fn row_to_view(row: sqlx::postgres::PgRow) -> Result<SettlementView, sqlx::Error> {
    let years: Decimal = row.try_get("years_of_service")?;
    let years_of_service = years.to_f64().unwrap_or(0.0);
    Ok(SettlementView {
        id: row.try_get("id")?,
        employee_id: row.try_get("employee_id")?,
        employee_code: row.try_get("employee_code")?,
        employee_name: format!(
            "{} {}",
            row.try_get::<String, _>("first_name_th")?,
            row.try_get::<String, _>("last_name_th")?
        ),
        termination_date: row.try_get("termination_date")?,
        unused_leave_days: row.try_get("unused_leave_days")?,
        notice_days: row.try_get("notice_days")?,
        include_severance: row.try_get("include_severance")?,
        years_of_service,
        last_month_pay: row.try_get("last_month_pay")?,
        leave_pay: row.try_get("leave_pay")?,
        notice_pay: row.try_get("notice_pay")?,
        severance_pay: row.try_get("severance_pay")?,
        total_gross: row.try_get("total_gross")?,
        breakdown: row.try_get("breakdown")?,
        status: row.try_get("status")?,
        created_at: row.try_get("created_at")?,
    })
}

fn validate(input: &SettlementInput) -> Result<(), SettlementError> {
    if input.unused_leave_days < 0 {
        return Err(SettlementError::NegativeDays("unused_leave_days"));
    }
    if input.notice_days < 0 {
        return Err(SettlementError::NegativeDays("notice_days"));
    }
    Ok(())
}

fn severance_days_for(years: f64) -> i32 {
    SeveranceTier::default_tiers()
        .into_iter()
        .find(|tier| match tier.max_years {
            Some(max) => years >= tier.min_years && years < max,
            None => years >= tier.min_years,
        })
        .map(|t| t.days)
        .unwrap_or(0)
}
