//! Performance review cycles: one review per active employee, HR ratings while
//! the cycle is active, and employee self-reads. Tenant isolation is
//! `begin_tenant` + explicit `tenant_id` predicates.

use chrono::{DateTime, NaiveDate, Utc};
use serde::Serialize;
use sqlx::Row;
use talentos_db::TenantPool;
use talentos_kernel::TenantId;
use uuid::Uuid;

#[derive(Debug, thiserror::Error)]
pub enum PerformanceError {
    #[error("review cycle not found")]
    CycleNotFound,
    #[error("review not found")]
    ReviewNotFound,
    #[error("cycle name must be 1-200 characters")]
    InvalidName,
    #[error("period end must not precede the start")]
    InvalidPeriod,
    #[error("rating must be between 1 and 5")]
    InvalidRating,
    #[error("strengths and improvements must be at most 2000 characters")]
    InvalidText,
    #[error("cycle must be in draft status")]
    CycleNotDraft,
    #[error("cycle must be active")]
    CycleNotActive,
    #[error("review has not been submitted yet")]
    ReviewNotSubmitted,
    #[error("caller is not the direct manager of this employee")]
    NotManager,
    #[error("employee must not be their own manager")]
    SelfAssignment,
    #[error("manager must belong to this tenant")]
    ManagerNotFound,
    #[error("database error: {0}")]
    Db(#[from] sqlx::Error),
}

#[derive(Debug, Clone)]
pub struct CycleInput {
    pub name: String,
    pub period_start: NaiveDate,
    pub period_end: NaiveDate,
}

#[derive(Debug, Serialize)]
pub struct CycleView {
    pub id: Uuid,
    pub name: String,
    pub period_start: NaiveDate,
    pub period_end: NaiveDate,
    pub status: String,
    pub review_count: i64,
    pub submitted_count: i64,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Serialize)]
pub struct ReviewView {
    pub id: Uuid,
    pub cycle_id: Uuid,
    pub cycle_name: String,
    pub period_start: NaiveDate,
    pub period_end: NaiveDate,
    pub employee_id: Uuid,
    pub employee_code: String,
    pub employee_name: String,
    pub status: String,
    pub rating: Option<i32>,
    pub strengths: Option<String>,
    pub improvements: Option<String>,
    pub submitted_at: Option<DateTime<Utc>>,
    pub acknowledged_at: Option<DateTime<Utc>>,
}

fn map_cycle(row: &sqlx::postgres::PgRow) -> Result<CycleView, sqlx::Error> {
    Ok(CycleView {
        id: row.try_get("id")?,
        name: row.try_get("name")?,
        period_start: row.try_get("period_start")?,
        period_end: row.try_get("period_end")?,
        status: row.try_get("status")?,
        review_count: row.try_get("review_count")?,
        submitted_count: row.try_get("submitted_count")?,
        created_at: row.try_get("created_at")?,
    })
}

fn map_review(row: &sqlx::postgres::PgRow) -> Result<ReviewView, sqlx::Error> {
    Ok(ReviewView {
        id: row.try_get("id")?,
        cycle_id: row.try_get("cycle_id")?,
        cycle_name: row.try_get("cycle_name")?,
        period_start: row.try_get("period_start")?,
        period_end: row.try_get("period_end")?,
        employee_id: row.try_get("employee_id")?,
        employee_code: row.try_get("employee_code")?,
        employee_name: format!(
            "{} {}",
            row.try_get::<String, _>("first_name_th")?,
            row.try_get::<String, _>("last_name_th")?
        ),
        status: row.try_get("status")?,
        rating: row.try_get("rating")?,
        strengths: row.try_get("strengths")?,
        improvements: row.try_get("improvements")?,
        submitted_at: row.try_get("submitted_at")?,
        acknowledged_at: row.try_get("acknowledged_at")?,
    })
}

const REVIEW_SELECT: &str = r#"SELECT r.id, r.cycle_id, c.name AS cycle_name, c.period_start,
              c.period_end, r.employee_id, e.employee_code, e.first_name_th, e.last_name_th,
              r.status, r.rating, r.strengths, r.improvements, r.submitted_at, r.acknowledged_at
         FROM performance_reviews r
         JOIN performance_cycles c ON c.id = r.cycle_id AND c.tenant_id = r.tenant_id
         JOIN employees e ON e.id = r.employee_id AND e.tenant_id = r.tenant_id
        WHERE r.tenant_id = $1"#;

pub async fn list_cycles(
    pool: &TenantPool,
    tenant_id: TenantId,
) -> Result<Vec<CycleView>, PerformanceError> {
    let mut tx = pool.begin_tenant(tenant_id).await?;
    let rows = sqlx::query(
        r#"SELECT c.id, c.name, c.period_start, c.period_end, c.status, c.created_at,
                  COUNT(r.id) AS review_count,
                  COUNT(r.id) FILTER (WHERE r.status = 'submitted') AS submitted_count
             FROM performance_cycles c
             LEFT JOIN performance_reviews r
                    ON r.cycle_id = c.id AND r.tenant_id = c.tenant_id
            WHERE c.tenant_id = $1
            GROUP BY c.id
            ORDER BY (c.status = 'active') DESC, c.created_at DESC"#,
    )
    .bind(tenant_id.0)
    .fetch_all(&mut *tx)
    .await?;
    tx.commit().await?;
    rows.iter()
        .map(|row| map_cycle(row).map_err(PerformanceError::from))
        .collect()
}

pub async fn create_cycle(
    pool: &TenantPool,
    tenant_id: TenantId,
    user_id: Uuid,
    input: &CycleInput,
) -> Result<CycleView, PerformanceError> {
    let name = input.name.trim();
    if name.is_empty() || name.chars().count() > 200 {
        return Err(PerformanceError::InvalidName);
    }
    if input.period_end < input.period_start {
        return Err(PerformanceError::InvalidPeriod);
    }
    let mut tx = pool.begin_tenant(tenant_id).await?;
    let row = sqlx::query(
        r#"INSERT INTO performance_cycles
             (tenant_id, name, period_start, period_end, created_by)
           VALUES ($1,$2,$3,$4,$5)
           RETURNING id, name, period_start, period_end, status, created_at,
                     0::bigint AS review_count, 0::bigint AS submitted_count"#,
    )
    .bind(tenant_id.0)
    .bind(name)
    .bind(input.period_start)
    .bind(input.period_end)
    .bind(user_id)
    .fetch_one(&mut *tx)
    .await?;
    let view = map_cycle(&row)?;
    tx.commit().await?;
    Ok(view)
}

pub async fn start_cycle(
    pool: &TenantPool,
    tenant_id: TenantId,
    cycle_id: Uuid,
) -> Result<(), PerformanceError> {
    let mut tx = pool.begin_tenant(tenant_id).await?;
    let status: Option<String> = sqlx::query_scalar(
        "SELECT status FROM performance_cycles WHERE tenant_id = $1 AND id = $2 FOR UPDATE",
    )
    .bind(tenant_id.0)
    .bind(cycle_id)
    .fetch_optional(&mut *tx)
    .await?;
    match status.as_deref() {
        None => return Err(PerformanceError::CycleNotFound),
        Some("draft") => {}
        Some(_) => return Err(PerformanceError::CycleNotDraft),
    }

    sqlx::query(
        r#"INSERT INTO performance_reviews (tenant_id, cycle_id, employee_id)
           SELECT $1, $2, e.id FROM employees e
            WHERE e.tenant_id = $1 AND e.status <> 'terminated'
           ON CONFLICT (cycle_id, employee_id) DO NOTHING"#,
    )
    .bind(tenant_id.0)
    .bind(cycle_id)
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        "UPDATE performance_cycles SET status = 'active', updated_at = now()
                  WHERE tenant_id = $1 AND id = $2",
    )
    .bind(tenant_id.0)
    .bind(cycle_id)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(())
}

pub async fn close_cycle(
    pool: &TenantPool,
    tenant_id: TenantId,
    cycle_id: Uuid,
) -> Result<(), PerformanceError> {
    let mut tx = pool.begin_tenant(tenant_id).await?;
    let updated = sqlx::query(
        "UPDATE performance_cycles SET status = 'closed', updated_at = now()
          WHERE tenant_id = $1 AND id = $2 AND status = 'active'",
    )
    .bind(tenant_id.0)
    .bind(cycle_id)
    .execute(&mut *tx)
    .await?;
    if updated.rows_affected() == 0 {
        let exists: bool = sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM performance_cycles WHERE tenant_id = $1 AND id = $2)",
        )
        .bind(tenant_id.0)
        .bind(cycle_id)
        .fetch_one(&mut *tx)
        .await?;
        return if exists {
            Err(PerformanceError::CycleNotActive)
        } else {
            Err(PerformanceError::CycleNotFound)
        };
    }
    tx.commit().await?;
    Ok(())
}

pub async fn delete_cycle(
    pool: &TenantPool,
    tenant_id: TenantId,
    cycle_id: Uuid,
) -> Result<(), PerformanceError> {
    let mut tx = pool.begin_tenant(tenant_id).await?;
    // Only drafts are disposable: active/closed reviews are retained records.
    let deleted = sqlx::query(
        "DELETE FROM performance_cycles
          WHERE tenant_id = $1 AND id = $2 AND status = 'draft'",
    )
    .bind(tenant_id.0)
    .bind(cycle_id)
    .execute(&mut *tx)
    .await?;
    if deleted.rows_affected() == 0 {
        let exists: bool = sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM performance_cycles WHERE tenant_id = $1 AND id = $2)",
        )
        .bind(tenant_id.0)
        .bind(cycle_id)
        .fetch_one(&mut *tx)
        .await?;
        return if exists {
            Err(PerformanceError::CycleNotDraft)
        } else {
            Err(PerformanceError::CycleNotFound)
        };
    }
    tx.commit().await?;
    Ok(())
}

pub async fn list_reviews(
    pool: &TenantPool,
    tenant_id: TenantId,
    cycle_id: Uuid,
) -> Result<Vec<ReviewView>, PerformanceError> {
    let mut tx = pool.begin_tenant(tenant_id).await?;
    let exists: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM performance_cycles WHERE tenant_id = $1 AND id = $2)",
    )
    .bind(tenant_id.0)
    .bind(cycle_id)
    .fetch_one(&mut *tx)
    .await?;
    if !exists {
        return Err(PerformanceError::CycleNotFound);
    }
    let sql = format!("{REVIEW_SELECT} AND r.cycle_id = $2 ORDER BY e.employee_code");
    let rows = sqlx::query(&sql)
        .bind(tenant_id.0)
        .bind(cycle_id)
        .fetch_all(&mut *tx)
        .await?;
    tx.commit().await?;
    rows.iter()
        .map(|row| map_review(row).map_err(PerformanceError::from))
        .collect()
}

pub async fn submit_review(
    pool: &TenantPool,
    tenant_id: TenantId,
    user_id: Uuid,
    review_id: Uuid,
    rating: i32,
    strengths: Option<&str>,
    improvements: Option<&str>,
) -> Result<ReviewView, PerformanceError> {
    if !(1..=5).contains(&rating) {
        return Err(PerformanceError::InvalidRating);
    }
    let strengths = strengths.map(str::trim).filter(|v| !v.is_empty());
    let improvements = improvements.map(str::trim).filter(|v| !v.is_empty());
    if strengths.is_some_and(|v| v.chars().count() > 2000)
        || improvements.is_some_and(|v| v.chars().count() > 2000)
    {
        return Err(PerformanceError::InvalidText);
    }

    let mut tx = pool.begin_tenant(tenant_id).await?;
    let status: Option<String> = sqlx::query_scalar(
        r#"SELECT c.status FROM performance_reviews r
             JOIN performance_cycles c ON c.id = r.cycle_id AND c.tenant_id = r.tenant_id
            WHERE r.tenant_id = $1 AND r.id = $2 FOR UPDATE"#,
    )
    .bind(tenant_id.0)
    .bind(review_id)
    .fetch_optional(&mut *tx)
    .await?;
    match status.as_deref() {
        None => return Err(PerformanceError::ReviewNotFound),
        Some("active") => {}
        Some(_) => return Err(PerformanceError::CycleNotActive),
    }

    sqlx::query(
        r#"UPDATE performance_reviews
              SET status = 'submitted', rating = $3, strengths = $4, improvements = $5,
                  reviewer_user_id = $6, submitted_at = now(),
                  acknowledged_at = NULL, updated_at = now()
            WHERE tenant_id = $1 AND id = $2"#,
    )
    .bind(tenant_id.0)
    .bind(review_id)
    .bind(rating)
    .bind(strengths)
    .bind(improvements)
    .bind(user_id)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;

    // Re-read through the joined view so the response matches list_reviews.
    let mut tx = pool.begin_tenant(tenant_id).await?;
    let sql = format!("{REVIEW_SELECT} AND r.id = $2");
    let row = sqlx::query(&sql)
        .bind(tenant_id.0)
        .bind(review_id)
        .fetch_one(&mut *tx)
        .await?;
    let view = map_review(&row)?;
    tx.commit().await?;
    Ok(view)
}

pub async fn list_my_reviews(
    pool: &TenantPool,
    tenant_id: TenantId,
    user_id: Uuid,
) -> Result<Vec<ReviewView>, PerformanceError> {
    let mut tx = pool.begin_tenant(tenant_id).await?;
    let sql = format!("{REVIEW_SELECT} AND e.user_id = $2 ORDER BY c.period_start DESC");
    let rows = sqlx::query(&sql)
        .bind(tenant_id.0)
        .bind(user_id)
        .fetch_all(&mut *tx)
        .await?;
    tx.commit().await?;
    rows.iter()
        .map(|row| map_review(row).map_err(PerformanceError::from))
        .collect()
}

pub async fn acknowledge_review(
    pool: &TenantPool,
    tenant_id: TenantId,
    user_id: Uuid,
    review_id: Uuid,
) -> Result<ReviewView, PerformanceError> {
    let mut tx = pool.begin_tenant(tenant_id).await?;
    let status: Option<String> = sqlx::query_scalar(
        r#"SELECT r.status FROM performance_reviews r
             JOIN employees e ON e.id = r.employee_id AND e.tenant_id = r.tenant_id
            WHERE r.tenant_id = $1 AND r.id = $2 AND e.user_id = $3
            FOR UPDATE"#,
    )
    .bind(tenant_id.0)
    .bind(review_id)
    .bind(user_id)
    .fetch_optional(&mut *tx)
    .await?;
    match status.as_deref() {
        None => return Err(PerformanceError::ReviewNotFound),
        Some("submitted") | Some("acknowledged") => {}
        Some(_) => return Err(PerformanceError::ReviewNotSubmitted),
    }

    sqlx::query(
        "UPDATE performance_reviews
            SET status = 'acknowledged',
                acknowledged_at = COALESCE(acknowledged_at, now()),
                updated_at = now()
          WHERE tenant_id = $1 AND id = $2",
    )
    .bind(tenant_id.0)
    .bind(review_id)
    .execute(&mut *tx)
    .await?;
    let sql = format!("{REVIEW_SELECT} AND r.id = $2");
    let row = sqlx::query(&sql)
        .bind(tenant_id.0)
        .bind(review_id)
        .fetch_one(&mut *tx)
        .await?;
    let view = map_review(&row)?;
    tx.commit().await?;
    Ok(view)
}

/// Reporting line: assign or clear `manager_id` on an employee.
pub async fn set_employee_manager(
    pool: &TenantPool,
    tenant_id: TenantId,
    user_id: Uuid,
    employee_id: Uuid,
    manager_id: Option<Uuid>,
) -> Result<(), PerformanceError> {
    let mut tx = pool.begin_tenant(tenant_id).await?;
    let employee: Option<Uuid> =
        sqlx::query_scalar("SELECT id FROM employees WHERE tenant_id = $1 AND id = $2")
            .bind(tenant_id.0)
            .bind(employee_id)
            .fetch_optional(&mut *tx)
            .await?;
    if employee.is_none() {
        return Err(PerformanceError::ReviewNotFound);
    }
    if manager_id.is_some_and(|m| m == employee_id) {
        return Err(PerformanceError::SelfAssignment);
    }
    if let Some(manager_id) = manager_id {
        let manager: Option<Uuid> =
            sqlx::query_scalar("SELECT id FROM employees WHERE tenant_id = $1 AND id = $2")
                .bind(tenant_id.0)
                .bind(manager_id)
                .fetch_optional(&mut *tx)
                .await?;
        if manager.is_none() {
            return Err(PerformanceError::ManagerNotFound);
        }
    }
    sqlx::query(
        "UPDATE employees SET manager_id = $3, updated_at = now()
           WHERE tenant_id = $1 AND id = $2",
    )
    .bind(tenant_id.0)
    .bind(employee_id)
    .bind(manager_id)
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        r#"INSERT INTO audit_log
             (tenant_id, actor_type, actor_id, action, resource_type, resource_id, after_state)
           VALUES ($1, 'user', $2, 'employee.manager_set', 'employee', $3, $4)"#,
    )
    .bind(tenant_id.0)
    .bind(user_id.to_string())
    .bind(employee_id.to_string())
    .bind(serde_json::json!({ "manager_id": manager_id }))
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(())
}

#[derive(Debug, Serialize)]
pub struct DirectReportView {
    pub id: Uuid,
    pub employee_code: String,
    pub employee_name: String,
}

/// Employees whose `manager_id` points at the caller's employee record.
pub async fn list_direct_reports(
    pool: &TenantPool,
    tenant_id: TenantId,
    user_id: Uuid,
) -> Result<Vec<DirectReportView>, PerformanceError> {
    let mut tx = pool.begin_tenant(tenant_id).await?;
    let rows = sqlx::query(
        r#"SELECT e2.id, e2.employee_code, e2.first_name_th, e2.last_name_th
             FROM employees e1
             JOIN employees e2 ON e2.manager_id = e1.id AND e2.tenant_id = e1.tenant_id
            WHERE e1.tenant_id = $1 AND e1.user_id = $2
            ORDER BY e2.employee_code"#,
    )
    .bind(tenant_id.0)
    .bind(user_id)
    .fetch_all(&mut *tx)
    .await?;
    tx.commit().await?;
    rows.iter()
        .map(|row| {
            Ok(DirectReportView {
                id: row.try_get("id")?,
                employee_code: row.try_get("employee_code")?,
                employee_name: format!(
                    "{} {}",
                    row.try_get::<String, _>("first_name_th")?,
                    row.try_get::<String, _>("last_name_th")?
                ),
            })
        })
        .collect()
}

/// Cycles that hold at least one review for a direct report of the caller.
pub async fn list_cycles_for_manager(
    pool: &TenantPool,
    tenant_id: TenantId,
    user_id: Uuid,
) -> Result<Vec<CycleView>, PerformanceError> {
    let mut tx = pool.begin_tenant(tenant_id).await?;
    let rows = sqlx::query(
        r#"SELECT c.id, c.name, c.period_start, c.period_end, c.status, c.created_at,
                  COUNT(r.id) AS review_count,
                  COUNT(r.id) FILTER (WHERE r.status = 'submitted') AS submitted_count
             FROM performance_cycles c
             JOIN performance_reviews r ON r.cycle_id = c.id AND r.tenant_id = c.tenant_id
             JOIN employees e1 ON e1.tenant_id = c.tenant_id AND e1.user_id = $2
             JOIN employees e2 ON e2.id = r.employee_id
                                AND e2.tenant_id = c.tenant_id
                                AND e2.manager_id = e1.id
            WHERE c.tenant_id = $1
            GROUP BY c.id
            ORDER BY (c.status = 'active') DESC, c.created_at DESC"#,
    )
    .bind(tenant_id.0)
    .bind(user_id)
    .fetch_all(&mut *tx)
    .await?;
    tx.commit().await?;
    rows.iter()
        .map(|row| map_cycle(row).map_err(PerformanceError::from))
        .collect()
}

#[derive(Debug, Serialize)]
pub struct CalibrationView {
    pub id: Uuid,
    pub cycle_id: Uuid,
    pub employee_id: Uuid,
    pub employee_code: String,
    pub employee_name: String,
    pub calibrated_rating: i32,
    pub notes: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

fn map_calibration(row: &sqlx::postgres::PgRow) -> Result<CalibrationView, sqlx::Error> {
    Ok(CalibrationView {
        id: row.try_get("id")?,
        cycle_id: row.try_get("cycle_id")?,
        employee_id: row.try_get("employee_id")?,
        employee_code: row.try_get("employee_code")?,
        employee_name: format!(
            "{} {}",
            row.try_get::<String, _>("first_name_th")?,
            row.try_get::<String, _>("last_name_th")?
        ),
        calibrated_rating: row.try_get("calibrated_rating")?,
        notes: row.try_get("notes")?,
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
    })
}

const CALIBRATION_SELECT: &str = r#"SELECT b.id, b.cycle_id, b.employee_id, e.employee_code,
               e.first_name_th, e.last_name_th, b.calibrated_rating, b.notes,
               b.created_at, b.updated_at
          FROM performance_calibrations b
          JOIN employees e ON e.id = b.employee_id AND e.tenant_id = b.tenant_id
         WHERE b.tenant_id = $1"#;

/// Calibrations of a cycle. Callers with an employee record are scoped to
/// their own direct reports; callers without one (HR) see the whole cycle.
pub async fn list_calibrations(
    pool: &TenantPool,
    tenant_id: TenantId,
    user_id: Uuid,
    cycle_id: Uuid,
) -> Result<Vec<CalibrationView>, PerformanceError> {
    let mut tx = pool.begin_tenant(tenant_id).await?;
    let exists: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM performance_cycles WHERE tenant_id = $1 AND id = $2)",
    )
    .bind(tenant_id.0)
    .bind(cycle_id)
    .fetch_one(&mut *tx)
    .await?;
    if !exists {
        return Err(PerformanceError::CycleNotFound);
    }
    let is_manager: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM employees WHERE tenant_id = $1 AND user_id = $2)",
    )
    .bind(tenant_id.0)
    .bind(user_id)
    .fetch_one(&mut *tx)
    .await?;
    let sql = if is_manager {
        format!(
            "{CALIBRATION_SELECT}
              AND b.cycle_id = $2
              AND b.employee_id IN (
                    SELECT e2.id FROM employees e1
                    JOIN employees e2 ON e2.manager_id = e1.id AND e2.tenant_id = e1.tenant_id
                   WHERE e1.tenant_id = $1 AND e1.user_id = $3)"
        )
    } else {
        format!("{CALIBRATION_SELECT} AND b.cycle_id = $2")
    };
    let mut builder = sqlx::query(&sql).bind(tenant_id.0);
    if is_manager {
        builder = builder.bind(cycle_id).bind(user_id);
    } else {
        builder = builder.bind(cycle_id);
    }
    let rows = builder.fetch_all(&mut *tx).await?;
    tx.commit().await?;
    rows.iter()
        .map(|row| map_calibration(row).map_err(PerformanceError::from))
        .collect()
}

/// Insert or override the calibration of `employee_id` in `cycle_id`.
/// Only the employee's direct manager (the caller's own employee record) may.
pub async fn upsert_calibration(
    pool: &TenantPool,
    tenant_id: TenantId,
    user_id: Uuid,
    cycle_id: Uuid,
    employee_id: Uuid,
    calibrated_rating: i32,
    notes: Option<&str>,
) -> Result<CalibrationView, PerformanceError> {
    if !(1..=5).contains(&calibrated_rating) {
        return Err(PerformanceError::InvalidRating);
    }
    let notes = notes.map(str::trim).filter(|v| !v.is_empty());
    if notes.is_some_and(|v| v.chars().count() > 2000) {
        return Err(PerformanceError::InvalidText);
    }

    let mut tx = pool.begin_tenant(tenant_id).await?;
    let caller: Uuid =
        sqlx::query_scalar("SELECT id FROM employees WHERE tenant_id = $1 AND user_id = $2")
            .bind(tenant_id.0)
            .bind(user_id)
            .fetch_optional(&mut *tx)
            .await?
            .ok_or(PerformanceError::NotManager)?;
    let allowed: Option<Uuid> =
        sqlx::query_scalar("SELECT manager_id FROM employees WHERE tenant_id = $1 AND id = $2")
            .bind(tenant_id.0)
            .bind(employee_id)
            .fetch_optional(&mut *tx)
            .await?;
    if allowed != Some(caller) {
        return Err(PerformanceError::NotManager);
    }
    let cycle: Option<Uuid> =
        sqlx::query_scalar("SELECT id FROM performance_cycles WHERE tenant_id = $1 AND id = $2")
            .bind(tenant_id.0)
            .bind(cycle_id)
            .fetch_optional(&mut *tx)
            .await?;
    if cycle.is_none() {
        return Err(PerformanceError::CycleNotFound);
    }
    let row = sqlx::query(
        r#"INSERT INTO performance_calibrations
             (tenant_id, cycle_id, employee_id, manager_id, calibrated_rating, notes)
            VALUES ($1, $2, $3, $4, $5, $6)
            ON CONFLICT (tenant_id, cycle_id, employee_id) DO UPDATE
                SET calibrated_rating = EXCLUDED.calibrated_rating,
                    notes = EXCLUDED.notes,
                    updated_at = now()
            RETURNING id"#,
    )
    .bind(tenant_id.0)
    .bind(cycle_id)
    .bind(employee_id)
    .bind(caller)
    .bind(calibrated_rating)
    .bind(notes)
    .fetch_one(&mut *tx)
    .await?;
    let calibration_id: Uuid = row.try_get("id")?;
    sqlx::query(
        r#"INSERT INTO audit_log
             (tenant_id, actor_type, actor_id, action, resource_type, resource_id, after_state)
           VALUES ($1, 'user', $2, 'performance.calibration', 'performance_calibration', $3, $4)"#,
    )
    .bind(tenant_id.0)
    .bind(user_id.to_string())
    .bind(calibration_id.to_string())
    .bind(serde_json::json!({
        "cycle_id": cycle_id,
        "employee_id": employee_id,
        "calibrated_rating": calibrated_rating,
        "notes": notes,
    }))
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;

    // Re-read through the joined view so the response matches list_calibrations.
    let mut tx = pool.begin_tenant(tenant_id).await?;
    let sql = format!("{CALIBRATION_SELECT} AND b.id = $2");
    let row = sqlx::query(&sql)
        .bind(tenant_id.0)
        .bind(calibration_id)
        .fetch_one(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(map_calibration(&row)?)
}
