//! OT (overtime) self-service requests.
//!
//! This slice only authorizes OT; payroll still derives OT from attendance,
//! and auto-applying approved OT to attendance/payroll is a future slice.

use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use sqlx::Row;
use talentos_db::TenantPool;
use talentos_kernel::TenantId;
use uuid::Uuid;

const REASON_MAX_CHARS: usize = 500;
const VALID_STATUSES: &[&str] = &["pending", "approved", "rejected", "cancelled"];

#[derive(Debug, thiserror::Error)]
pub enum OtError {
    #[error("employee not found for this tenant")]
    EmployeeNotFound,
    #[error("OT request not found")]
    NotFound,
    #[error("OT request is not in pending status")]
    NotPending,
    #[error("a request already exists for this work date")]
    DuplicateDate,
    #[error("minutes must be between 1 and 720")]
    InvalidMinutes,
    #[error("reason must be at most 500 characters")]
    InvalidReason,
    #[error("invalid status filter")]
    InvalidStatus,
    #[error("database error: {0}")]
    Db(#[from] sqlx::Error),
}

#[derive(Debug, Clone, Deserialize)]
pub struct OtInput {
    pub work_date: NaiveDate,
    pub minutes: i32,
    pub reason: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct OtRequestView {
    pub id: Uuid,
    pub employee_id: Uuid,
    pub work_date: NaiveDate,
    pub minutes: i32,
    pub reason: Option<String>,
    pub status: String,
}

#[derive(Debug, Serialize)]
pub struct PendingOt {
    pub id: Uuid,
    pub employee_id: Uuid,
    pub employee_code: String,
    pub employee_name: String,
    pub work_date: NaiveDate,
    pub minutes: i32,
    pub reason: Option<String>,
    pub status: String,
    pub decided_at: Option<DateTime<Utc>>,
}

fn validate_reason(reason: &Option<String>) -> Result<(), OtError> {
    if let Some(ref r) = reason {
        if r.chars().count() > REASON_MAX_CHARS {
            return Err(OtError::InvalidReason);
        }
    }
    Ok(())
}

/// Create an OT request for the caller's linked employee.
pub async fn create_request(
    pool: &TenantPool,
    tenant_id: TenantId,
    caller_user_id: Uuid,
    input: OtInput,
) -> Result<OtRequestView, OtError> {
    if input.minutes < 1 || input.minutes > 720 {
        return Err(OtError::InvalidMinutes);
    }
    validate_reason(&input.reason)?;

    let mut tx = pool.begin_tenant(tenant_id).await?;

    let employee = sqlx::query(
        "SELECT id, entity_id FROM employees WHERE tenant_id = $1 AND user_id = $2 LIMIT 1",
    )
    .bind(tenant_id.0)
    .bind(caller_user_id)
    .fetch_optional(&mut *tx)
    .await?;

    let employee = employee.ok_or(OtError::EmployeeNotFound)?;
    let employee_id: Uuid = employee.try_get("id")?;
    let entity_id: Uuid = employee.try_get("entity_id")?;

    let row = sqlx::query(
        r#"INSERT INTO ot_requests
             (tenant_id, entity_id, employee_id, work_date, minutes, reason, status, requested_by)
           VALUES ($1,$2,$3,$4,$5,$6,'pending',$7)
           RETURNING id, status"#,
    )
    .bind(tenant_id.0)
    .bind(entity_id)
    .bind(employee_id)
    .bind(input.work_date)
    .bind(input.minutes)
    .bind(&input.reason)
    .bind(caller_user_id)
    .fetch_one(&mut *tx)
    .await
    .map_err(|e| match e {
        sqlx::Error::Database(ref db)
            if db.constraint() == Some("idx_ot_requests_unique_live_date") =>
        {
            OtError::DuplicateDate
        }
        _ => OtError::Db(e),
    })?;

    tx.commit().await?;

    Ok(OtRequestView {
        id: row.try_get("id")?,
        employee_id,
        work_date: input.work_date,
        minutes: input.minutes,
        reason: input.reason,
        status: row.try_get("status")?,
    })
}

/// List OT requests for one employee, optionally filtered by status.
/// shortcut: history is capped at 200 rows with no pagination.
pub async fn list_requests(
    pool: &TenantPool,
    tenant_id: TenantId,
    employee_id: Uuid,
    status: Option<&str>,
) -> Result<Vec<OtRequestView>, OtError> {
    let mut tx = pool.begin_tenant(tenant_id).await?;
    let rows = sqlx::query(
        r#"SELECT id, employee_id, work_date, minutes, reason, status
           FROM ot_requests
           WHERE tenant_id = $1 AND employee_id = $2
             AND ($3::text IS NULL OR status = $3)
           ORDER BY (status = 'pending') DESC, work_date DESC
           LIMIT 200"#,
    )
    .bind(tenant_id.0)
    .bind(employee_id)
    .bind(status)
    .fetch_all(&mut *tx)
    .await?;
    tx.commit().await?;

    rows.into_iter()
        .map(|row| {
            Ok(OtRequestView {
                id: row.try_get("id")?,
                employee_id: row.try_get("employee_id")?,
                work_date: row.try_get("work_date")?,
                minutes: row.try_get("minutes")?,
                reason: row.try_get("reason")?,
                status: row.try_get("status")?,
            })
        })
        .collect::<Result<Vec<_>, sqlx::Error>>()
        .map_err(Into::into)
}

/// Cancel an OT request. Only the owning employee may cancel, and only
/// requests in `pending` status.
pub async fn cancel_request(
    pool: &TenantPool,
    tenant_id: TenantId,
    user_id: Uuid,
    request_id: Uuid,
) -> Result<(), OtError> {
    let employee_id = match employee_of_user(pool, tenant_id, user_id).await? {
        Some(id) => id,
        None => return Err(OtError::NotFound),
    };

    let mut tx = pool.begin_tenant(tenant_id).await?;

    let updated = sqlx::query(
        r#"UPDATE ot_requests
           SET status = 'cancelled', updated_at = now()
           WHERE id = $1 AND tenant_id = $2 AND employee_id = $3 AND status = 'pending'
           RETURNING status"#,
    )
    .bind(request_id)
    .bind(tenant_id.0)
    .bind(employee_id)
    .fetch_optional(&mut *tx)
    .await?;

    if updated.is_some() {
        tx.commit().await?;
        return Ok(());
    }

    let row = sqlx::query(
        r#"SELECT status, employee_id FROM ot_requests
           WHERE id = $1 AND tenant_id = $2"#,
    )
    .bind(request_id)
    .bind(tenant_id.0)
    .fetch_optional(&mut *tx)
    .await?;

    let _ = tx.rollback().await;
    match row {
        Some(r) => {
            let owner: Uuid = r.try_get("employee_id")?;
            if owner != employee_id {
                Err(OtError::NotFound)
            } else {
                Err(OtError::NotPending)
            }
        }
        None => Err(OtError::NotFound),
    }
}

/// Tenant-wide OT requests for HR/manager views, optionally filtered by status
/// and work-date range. Validates `status` and clamps `limit` to 1..=500.
pub async fn tenant_requests(
    pool: &TenantPool,
    tenant_id: TenantId,
    status: Option<&str>,
    from: Option<NaiveDate>,
    to: Option<NaiveDate>,
    limit: i64,
) -> Result<Vec<PendingOt>, OtError> {
    if let Some(s) = status {
        if !VALID_STATUSES.contains(&s) {
            return Err(OtError::InvalidStatus);
        }
    }
    let limit = limit.clamp(1, 500);

    let mut tx = pool.begin_tenant(tenant_id).await?;
    let rows = sqlx::query(
        r#"SELECT o.id, o.employee_id, e.employee_code, e.first_name_th, e.last_name_th,
                  o.work_date, o.minutes, o.reason, o.status, o.decided_at
           FROM ot_requests o
           JOIN employees e ON e.id = o.employee_id
           WHERE o.tenant_id = $1
             AND ($2::text IS NULL OR o.status = $2)
             AND ($3::date IS NULL OR o.work_date >= $3)
             AND ($4::date IS NULL OR o.work_date <= $4)
           ORDER BY o.work_date DESC, o.created_at DESC
           LIMIT $5"#,
    )
    .bind(tenant_id.0)
    .bind(status)
    .bind(from)
    .bind(to)
    .bind(limit)
    .fetch_all(&mut *tx)
    .await?;
    tx.commit().await?;

    rows.into_iter()
        .map(|row| {
            Ok(PendingOt {
                id: row.try_get("id")?,
                employee_id: row.try_get("employee_id")?,
                employee_code: row.try_get("employee_code")?,
                employee_name: format!(
                    "{} {}",
                    row.try_get::<String, _>("first_name_th")?,
                    row.try_get::<String, _>("last_name_th")?
                ),
                work_date: row.try_get("work_date")?,
                minutes: row.try_get("minutes")?,
                reason: row.try_get("reason")?,
                status: row.try_get("status")?,
                decided_at: row.try_get("decided_at")?,
            })
        })
        .collect::<Result<Vec<_>, sqlx::Error>>()
        .map_err(Into::into)
}

/// All pending OT requests across the tenant, newest first.
pub async fn pending_approvals(
    pool: &TenantPool,
    tenant_id: TenantId,
) -> Result<Vec<PendingOt>, OtError> {
    tenant_requests(pool, tenant_id, Some("pending"), None, None, 200).await
}

/// Approve or reject a pending OT request.
/// shortcut: approved OT is authorization only; payroll still uses attendance-derived OT,
/// and auto-apply to attendance/payroll is a future slice.
pub async fn decide_request(
    pool: &TenantPool,
    tenant_id: TenantId,
    approver_user_id: Uuid,
    request_id: Uuid,
    approve: bool,
) -> Result<String, OtError> {
    let mut tx = pool.begin_tenant(tenant_id).await?;

    let updated = sqlx::query(
        r#"UPDATE ot_requests
           SET status = $3, decided_by = $4, decided_at = now(), updated_at = now()
           WHERE id = $1 AND tenant_id = $2 AND status = 'pending'
           RETURNING status"#,
    )
    .bind(request_id)
    .bind(tenant_id.0)
    .bind(if approve { "approved" } else { "rejected" })
    .bind(approver_user_id)
    .fetch_optional(&mut *tx)
    .await?;

    if let Some(row) = updated {
        tx.commit().await?;
        return row.try_get("status").map_err(Into::into);
    }

    let row = sqlx::query("SELECT status FROM ot_requests WHERE id = $1 AND tenant_id = $2")
        .bind(request_id)
        .bind(tenant_id.0)
        .fetch_optional(&mut *tx)
        .await?;

    let _ = tx.rollback().await;
    match row {
        Some(_) => Err(OtError::NotPending),
        None => Err(OtError::NotFound),
    }
}

/// Employee record owned by a login user (self-service default).
pub async fn employee_of_user(
    pool: &TenantPool,
    tenant_id: TenantId,
    user_id: Uuid,
) -> Result<Option<Uuid>, OtError> {
    let mut tx = pool.begin_tenant(tenant_id).await?;
    let row = sqlx::query("SELECT id FROM employees WHERE tenant_id = $1 AND user_id = $2 LIMIT 1")
        .bind(tenant_id.0)
        .bind(user_id)
        .fetch_optional(&mut *tx)
        .await?;
    tx.commit().await?;
    row.map(|r| r.try_get("id")).transpose().map_err(Into::into)
}
