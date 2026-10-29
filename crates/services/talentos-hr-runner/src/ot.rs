//! OT (overtime) self-service requests.
//!
//! Approved OT requests raise the effective `ot_minutes` of the attendance day
//! to `max(derived_ot_minutes, approved request minutes)`. Rejection or cancel
//! restores the derived value when no other approved request exists.

use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use sqlx::Row;
use talentos_db::TenantPool;
use talentos_kernel::TenantId;
use uuid::Uuid;

use crate::attendance;

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
    #[error("approver is not authorised for this step")]
    Forbidden,
    #[error("a request already exists for this work date")]
    DuplicateDate,
    #[error("minutes must be between 1 and 720")]
    InvalidMinutes,
    #[error("reason must be at most 500 characters")]
    InvalidReason,
    #[error("invalid status filter")]
    InvalidStatus,
    #[error("attendance day is already approved; revoke approval before changing OT")]
    DayAlreadyApproved,
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
    pub step_no: i32,
    pub total_steps: i64,
    pub approver_role: String,
}

/// Result of approving/rejecting an OT request.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct OtDecideOutcome {
    pub status: String,
    pub step: i32,
    pub total_steps: i64,
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

    let request_id: Uuid = row.try_get("id")?;

    // Snapshot the tenant's OT approval chain into steps (default single HR step).
    let chain: Vec<String> = sqlx::query_scalar(
        "SELECT ot_approval_chain FROM tenant_payroll_settings WHERE tenant_id = $1",
    )
    .bind(tenant_id.0)
    .fetch_optional(&mut *tx)
    .await?
    .unwrap_or_else(|| vec!["hr_admin".to_string()]);

    for (idx, role) in normalise_chain(chain).iter().enumerate() {
        sqlx::query(
            r#"INSERT INTO ot_approval_steps
                 (tenant_id, ot_request_id, step_no, approver_role, status)
               VALUES ($1,$2,$3,$4,'pending')"#,
        )
        .bind(tenant_id.0)
        .bind(request_id)
        .bind(i32::try_from(idx + 1).unwrap_or(1))
        .bind(role)
        .execute(&mut *tx)
        .await?;
    }

    tx.commit().await?;

    Ok(OtRequestView {
        id: request_id,
        employee_id,
        work_date: input.work_date,
        minutes: input.minutes,
        reason: input.reason,
        status: row.try_get("status")?,
    })
}

/// Coerce a missing or invalid OT chain to the safe one-step HR default.
fn normalise_chain(chain: Vec<String>) -> Vec<String> {
    let valid = ["manager", "hr_admin"];
    let chain: Vec<String> = chain
        .into_iter()
        .filter(|r| valid.contains(&r.as_str()))
        .collect();
    let unique: std::collections::HashSet<_> = chain.iter().collect();
    if chain.is_empty()
        || chain.len() > 3
        || unique.len() != chain.len()
        || chain.last().map(String::as_str) != Some("hr_admin")
    {
        vec!["hr_admin".to_string()]
    } else {
        chain
    }
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
           RETURNING status, work_date"#,
    )
    .bind(request_id)
    .bind(tenant_id.0)
    .bind(employee_id)
    .fetch_optional(&mut *tx)
    .await?;

    if let Some(row) = updated {
        let work_date: NaiveDate = row.try_get("work_date")?;
        sqlx::query(
            "UPDATE ot_approval_steps SET status = 'skipped' WHERE ot_request_id = $1 AND status = 'pending'",
        )
        .bind(request_id)
        .execute(&mut *tx)
        .await?;
        attendance::reapply_ot_request(&mut tx, employee_id, work_date)
            .await
            .map_err(|e| match e {
                attendance::AttendanceError::DayAlreadyApproved => OtError::DayAlreadyApproved,
                attendance::AttendanceError::Db(sqlx_err) => OtError::Db(sqlx_err),
                _ => unreachable!("reapply_ot_request only returns Db or DayAlreadyApproved"),
            })?;
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
                  o.work_date, o.minutes, o.reason, o.status, o.decided_at,
                  COALESCE(
                      (SELECT step_no FROM ot_approval_steps
                        WHERE ot_request_id = o.id AND status = 'pending'
                        ORDER BY step_no LIMIT 1),
                      1
                  ) AS step_no,
                  COALESCE(
                      (SELECT approver_role FROM ot_approval_steps
                        WHERE ot_request_id = o.id AND status = 'pending'
                        ORDER BY step_no LIMIT 1),
                      'hr_admin'
                  ) AS approver_role,
                  GREATEST(
                      1,
                      (SELECT COUNT(*) FROM ot_approval_steps s2 WHERE s2.ot_request_id = o.id)
                  ) AS total_steps
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
                step_no: row.try_get("step_no")?,
                total_steps: row.try_get("total_steps")?,
                approver_role: row.try_get("approver_role")?,
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

/// Approve or reject a pending OT request, advancing the approval chain.
/// Final approval raises the attendance day's effective OT to
/// `max(derived, approved request minutes)`; rejection or cancel restores the
/// derived value. Days already approved reject changes.
pub async fn decide_request(
    pool: &TenantPool,
    tenant_id: TenantId,
    approver_user_id: Uuid,
    roles: &[String],
    request_id: Uuid,
    approve: bool,
) -> Result<OtDecideOutcome, OtError> {
    let mut tx = pool.begin_tenant(tenant_id).await?;

    let request = sqlx::query(
        r#"SELECT employee_id, work_date
           FROM ot_requests
           WHERE id = $1 AND tenant_id = $2 AND status = 'pending'
           FOR UPDATE"#,
    )
    .bind(request_id)
    .bind(tenant_id.0)
    .fetch_optional(&mut *tx)
    .await?;

    let Some(request) = request else {
        let row = sqlx::query("SELECT status FROM ot_requests WHERE id = $1 AND tenant_id = $2")
            .bind(request_id)
            .bind(tenant_id.0)
            .fetch_optional(&mut *tx)
            .await?;
        let _ = tx.rollback().await;
        return match row {
            Some(_) => Err(OtError::NotPending),
            None => Err(OtError::NotFound),
        };
    };

    let employee_id: Uuid = request.try_get("employee_id")?;
    let work_date: NaiveDate = request.try_get("work_date")?;

    // Current pending step, creating a single HR step on the fly for legacy
    // requests that pre-date the steps table.
    let step = sqlx::query(
        r#"SELECT id, step_no, approver_role
           FROM ot_approval_steps
           WHERE ot_request_id = $1 AND status = 'pending'
           ORDER BY step_no
           LIMIT 1
           FOR UPDATE"#,
    )
    .bind(request_id)
    .fetch_optional(&mut *tx)
    .await?;

    let (step_id, step_no, approver_role) = match step {
        Some(row) => {
            let id: i64 = row.try_get("id")?;
            let no: i32 = row.try_get("step_no")?;
            let role: String = row.try_get("approver_role")?;
            (id, no, role)
        }
        None => {
            let id: i64 = sqlx::query_scalar(
                r#"INSERT INTO ot_approval_steps
                     (tenant_id, ot_request_id, step_no, approver_role, status)
                   VALUES ($1,$2,1,'hr_admin','pending')
                   RETURNING id"#,
            )
            .bind(tenant_id.0)
            .bind(request_id)
            .fetch_one(&mut *tx)
            .await?;
            (id, 1, "hr_admin".to_string())
        }
    };

    let can_decide = roles.iter().any(|r| r == &approver_role || r == "hr_admin");
    if !can_decide {
        let _ = tx.rollback().await;
        return Err(OtError::Forbidden);
    }

    let total_steps: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM ot_approval_steps WHERE ot_request_id = $1")
            .bind(request_id)
            .fetch_one(&mut *tx)
            .await?;

    if approve {
        sqlx::query(
            r#"UPDATE ot_approval_steps
               SET status = 'approved', decided_by = $1, decided_at = now()
               WHERE id = $2"#,
        )
        .bind(approver_user_id)
        .bind(step_id)
        .execute(&mut *tx)
        .await?;

        let next_step: Option<i32> = sqlx::query_scalar(
            r#"SELECT step_no FROM ot_approval_steps
               WHERE ot_request_id = $1 AND status = 'pending'
               ORDER BY step_no
               LIMIT 1"#,
        )
        .bind(request_id)
        .fetch_optional(&mut *tx)
        .await?;

        if let Some(next) = next_step {
            tx.commit().await?;
            return Ok(OtDecideOutcome {
                status: "pending".to_string(),
                step: next,
                total_steps,
            });
        }

        sqlx::query(
            r#"UPDATE ot_requests
               SET status = 'approved', decided_by = $1, decided_at = now(), updated_at = now()
               WHERE id = $2"#,
        )
        .bind(approver_user_id)
        .bind(request_id)
        .execute(&mut *tx)
        .await?;

        attendance::reapply_ot_request(&mut tx, employee_id, work_date)
            .await
            .map_err(|e| match e {
                attendance::AttendanceError::DayAlreadyApproved => OtError::DayAlreadyApproved,
                attendance::AttendanceError::Db(sqlx_err) => OtError::Db(sqlx_err),
                _ => unreachable!("reapply_ot_request only returns Db or DayAlreadyApproved"),
            })?;

        tx.commit().await?;
        Ok(OtDecideOutcome {
            status: "approved".to_string(),
            step: total_steps as i32,
            total_steps,
        })
    } else {
        sqlx::query(
            r#"UPDATE ot_approval_steps
               SET status = 'rejected', decided_by = $1, decided_at = now()
               WHERE id = $2"#,
        )
        .bind(approver_user_id)
        .bind(step_id)
        .execute(&mut *tx)
        .await?;

        sqlx::query(
            r#"UPDATE ot_approval_steps
               SET status = 'skipped'
               WHERE ot_request_id = $1 AND status = 'pending'"#,
        )
        .bind(request_id)
        .execute(&mut *tx)
        .await?;

        sqlx::query(
            r#"UPDATE ot_requests
               SET status = 'rejected', decided_by = $1, decided_at = now(), updated_at = now()
               WHERE id = $2"#,
        )
        .bind(approver_user_id)
        .bind(request_id)
        .execute(&mut *tx)
        .await?;

        attendance::reapply_ot_request(&mut tx, employee_id, work_date)
            .await
            .map_err(|e| match e {
                attendance::AttendanceError::DayAlreadyApproved => OtError::DayAlreadyApproved,
                attendance::AttendanceError::Db(sqlx_err) => OtError::Db(sqlx_err),
                _ => unreachable!("reapply_ot_request only returns Db or DayAlreadyApproved"),
            })?;

        tx.commit().await?;
        Ok(OtDecideOutcome {
            status: "rejected".to_string(),
            step: step_no,
            total_steps,
        })
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
