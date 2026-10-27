use std::collections::HashMap;

use chrono::{Datelike, NaiveDate};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use sqlx::Row;
use talentos_db::TenantPool;
use talentos_kernel::TenantId;
use uuid::Uuid;

use crate::notify::{enqueue_leave_decided, enqueue_leave_submitted};

/// Leave types the platform recognises (global defaults live in `leave_types`).
pub const LEAVE_TYPES: &[&str] = &[
    "annual",
    "sick",
    "personal",
    "maternity",
    "paternity",
    "bereavement",
    "unpaid",
];

/// Roles allowed in a leave approval chain.
pub const APPROVAL_ROLES: &[&str] = &["manager", "hr_admin"];

#[derive(Debug, thiserror::Error)]
pub enum LeaveError {
    #[error("leave_type must be one of: annual, sick, personal, maternity, paternity, bereavement, unpaid")]
    UnknownType,
    #[error("end_date must be on or after start_date")]
    BadRange,
    #[error("a single request cannot exceed 365 days")]
    TooLong,
    #[error("this request overlaps an existing pending or approved leave ({0}..{1})")]
    Overlap(NaiveDate, NaiveDate),
    #[error("employee not found for this tenant")]
    EmployeeNotFound,
    #[error("employee is not active")]
    NotActive,
    #[error("insufficient {leave_type} balance: requested {requested}, remaining {remaining}")]
    InsufficientBalance {
        leave_type: String,
        requested: Decimal,
        remaining: Decimal,
    },
    #[error("leave request not found")]
    NotFound,
    #[error("leave request cannot be cancelled in its current status")]
    NotCancellable,
    #[error("approver is not authorised for this step")]
    Forbidden,
    #[error("database error: {0}")]
    Db(#[from] sqlx::Error),
}

#[derive(Debug, Clone, Deserialize)]
pub struct LeaveInput {
    pub leave_type: String,
    pub start_date: NaiveDate,
    pub end_date: NaiveDate,
    pub reason: Option<String>,
    /// Defaults to the caller's own employee record.
    pub employee_id: Option<Uuid>,
}

#[derive(Debug, Serialize)]
pub struct LeaveRequestView {
    pub id: Uuid,
    pub employee_id: Uuid,
    pub leave_type: String,
    pub start_date: NaiveDate,
    pub end_date: NaiveDate,
    pub days: Decimal,
    pub status: String,
    pub reason: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct LeaveBalanceView {
    pub leave_type: String,
    pub label_th: String,
    pub total_days: Decimal,
    pub used_days: Decimal,
    pub remaining_days: Decimal,
    /// True when HR has provisioned a leave_balances row for this employee/year.
    pub provisioned: bool,
}

/// Result of approving/rejecting a leave request.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DecideOutcome {
    pub status: String,
    pub step: i32,
    pub total_steps: i64,
}

/// Calendar days inclusive: 1 Oct → 3 Oct is 3 days.
pub fn count_leave_days(start: NaiveDate, end: NaiveDate) -> Result<Decimal, LeaveError> {
    if end < start {
        return Err(LeaveError::BadRange);
    }
    let days = (end - start).num_days() + 1;
    if days > 365 {
        return Err(LeaveError::TooLong);
    }
    Ok(Decimal::from(days))
}

/// Create a leave request for `employee_id` (or the caller when omitted).
pub async fn create_request(
    pool: &TenantPool,
    tenant_id: TenantId,
    caller_user_id: Uuid,
    input: LeaveInput,
) -> Result<LeaveRequestView, LeaveError> {
    if !LEAVE_TYPES.contains(&input.leave_type.as_str()) {
        return Err(LeaveError::UnknownType);
    }
    let days = count_leave_days(input.start_date, input.end_date)?;
    let mut tx = pool.begin_tenant(tenant_id).await?;

    // Resolve employee + ensure active in this tenant (RLS already scopes rows).
    // Lock the row to serialize concurrent creates for the same employee and
    // prevent double-spending the balance.
    let employee = sqlx::query(
        r#"SELECT id, entity_id, status FROM employees
           WHERE tenant_id = $1
             AND ($2::uuid IS NOT NULL AND id = $2::uuid OR $2::uuid IS NULL AND user_id = $3)
           LIMIT 1
           FOR UPDATE"#,
    )
    .bind(tenant_id.0)
    .bind(input.employee_id)
    .bind(caller_user_id)
    .fetch_optional(&mut *tx)
    .await?;

    let employee = employee.ok_or(LeaveError::EmployeeNotFound)?;
    let employee_id: Uuid = employee.try_get("id")?;
    let entity_id: Uuid = employee.try_get("entity_id")?;
    let status: String = employee.try_get("status")?;
    if status == "terminated" {
        return Err(LeaveError::NotActive);
    }

    // Overlap guard: pending or approved requests for the same employee.
    let overlap = sqlx::query(
        r#"SELECT start_date, end_date FROM leave_requests
           WHERE tenant_id = $1 AND employee_id = $2
             AND status IN ('pending', 'approved')
             AND start_date <= $3 AND end_date >= $4
           LIMIT 1"#,
    )
    .bind(tenant_id.0)
    .bind(employee_id)
    .bind(input.end_date)
    .bind(input.start_date)
    .fetch_optional(&mut *tx)
    .await?;
    if let Some(row) = overlap {
        let s: NaiveDate = row.try_get("start_date")?;
        let e: NaiveDate = row.try_get("end_date")?;
        return Err(LeaveError::Overlap(s, e));
    }

    // Balance check for paid leave types; unpaid leave is never blocked.
    if input.leave_type != "unpaid" {
        let balance = sqlx::query(
            r#"SELECT lb.total_days, lb.used_days
               FROM leave_balances lb
               JOIN leave_types lt ON lt.id = lb.leave_type_id
               WHERE lb.employee_id = $1
                 AND lb.year = $2
                 AND lt.name = $3
                 AND (lt.tenant_id IS NULL OR lt.tenant_id = $4)
               LIMIT 1"#,
        )
        .bind(employee_id)
        .bind(input.start_date.year())
        .bind(&input.leave_type)
        .bind(tenant_id.0)
        .fetch_optional(&mut *tx)
        .await?;

        if let Some(row) = balance {
            let total: Decimal = row.try_get("total_days")?;
            let used: Decimal = row.try_get("used_days")?;
            let remaining = total - used;
            if days > remaining {
                return Err(LeaveError::InsufficientBalance {
                    leave_type: input.leave_type.clone(),
                    requested: days,
                    remaining,
                });
            }
        }
        // No balance row means HR has not provisioned this type yet — allow the
        // request; the approver sees the raw request and decides.
    }

    // Debit the balance when a row exists; unpaid leave is never tracked.
    let mut balance_debited = false;
    if input.leave_type != "unpaid" {
        let debited = sqlx::query(
            r#"UPDATE leave_balances lb
               SET used_days = used_days + $1, updated_at = now()
               FROM leave_types lt
               WHERE lb.employee_id = $2
                 AND lb.year = $3
                 AND lt.id = lb.leave_type_id
                 AND lt.name = $4
                 AND (lt.tenant_id IS NULL OR lt.tenant_id = $5)"#,
        )
        .bind(days)
        .bind(employee_id)
        .bind(input.start_date.year())
        .bind(&input.leave_type)
        .bind(tenant_id.0)
        .execute(&mut *tx)
        .await?;
        balance_debited = debited.rows_affected() > 0;
    }

    let row = sqlx::query(
        r#"INSERT INTO leave_requests
             (tenant_id, entity_id, employee_id, leave_type, start_date, end_date, days,
              reason, status, requested_by, balance_debited)
           VALUES ($1,$2,$3,$4,$5,$6,$7,$8,'pending',$9,$10)
           RETURNING id, status"#,
    )
    .bind(tenant_id.0)
    .bind(entity_id)
    .bind(employee_id)
    .bind(&input.leave_type)
    .bind(input.start_date)
    .bind(input.end_date)
    .bind(days)
    .bind(&input.reason)
    .bind(caller_user_id)
    .bind(balance_debited)
    .fetch_one(&mut *tx)
    .await?;

    let request_id: Uuid = row.try_get("id")?;

    // Snapshot the tenant's current approval chain for this request.
    let chain: Vec<String> = sqlx::query_scalar(
        "SELECT leave_approval_chain FROM tenant_payroll_settings WHERE tenant_id = $1",
    )
    .bind(tenant_id.0)
    .fetch_optional(&mut *tx)
    .await?
    .unwrap_or_else(|| vec!["hr_admin".to_string()]);
    let chain = normalise_chain(chain);

    for (idx, role) in chain.iter().enumerate() {
        sqlx::query(
            r#"INSERT INTO leave_approval_steps
                 (id, tenant_id, leave_request_id, step_no, approver_role, status)
               VALUES ($1,$2,$3,$4,$5,'pending')"#,
        )
        .bind(Uuid::new_v4())
        .bind(tenant_id.0)
        .bind(request_id)
        .bind(i32::try_from(idx + 1).unwrap_or(1))
        .bind(role)
        .execute(&mut *tx)
        .await?;
    }

    let label_th: String = sqlx::query_scalar(
        "SELECT label_th FROM leave_types WHERE name = $1 AND (tenant_id IS NULL OR tenant_id = $2) ORDER BY (tenant_id IS NOT NULL) DESC, label_th LIMIT 1"
    )
    .bind(&input.leave_type)
    .bind(tenant_id.0)
    .fetch_optional(&mut *tx)
    .await?
    .unwrap_or_else(|| input.leave_type.clone());

    enqueue_leave_submitted(
        &mut tx,
        tenant_id.0,
        employee_id,
        &label_th,
        input.start_date,
        input.end_date,
        days,
    )
    .await?;

    tx.commit().await?;

    Ok(LeaveRequestView {
        id: request_id,
        employee_id,
        leave_type: input.leave_type,
        start_date: input.start_date,
        end_date: input.end_date,
        days,
        status: row.try_get("status")?,
        reason: input.reason,
    })
}

/// Balances for one employee (defaults to the caller).
pub async fn list_balances(
    pool: &TenantPool,
    tenant_id: TenantId,
    employee_id: Uuid,
    year: i32,
) -> Result<Vec<LeaveBalanceView>, LeaveError> {
    let mut tx = pool.begin_tenant(tenant_id).await?;
    let rows = sqlx::query(
        r#"SELECT lt.name AS leave_type, lt.label_th,
                  COALESCE(lb.total_days, lt.max_days_per_year) AS total_days,
                  COALESCE(lb.used_days, 0) AS used_days,
                  (lb.id IS NOT NULL) AS provisioned
           FROM leave_types lt
           LEFT JOIN leave_balances lb
             ON lb.leave_type_id = lt.id AND lb.employee_id = $1 AND lb.year = $2
           WHERE lt.tenant_id IS NULL OR lt.tenant_id = $3
           ORDER BY lt.id"#,
    )
    .bind(employee_id)
    .bind(year)
    .bind(tenant_id.0)
    .fetch_all(&mut *tx)
    .await?;
    tx.commit().await?;

    rows.into_iter()
        .map(|row| {
            let total: Decimal = row.try_get("total_days")?;
            let used: Decimal = row.try_get("used_days")?;
            Ok(LeaveBalanceView {
                leave_type: row.try_get("leave_type")?,
                label_th: row.try_get("label_th")?,
                total_days: total,
                used_days: used,
                remaining_days: total - used,
                provisioned: row.try_get("provisioned")?,
            })
        })
        .collect::<Result<Vec<_>, sqlx::Error>>()
        .map_err(Into::into)
}

/// Requests for one employee, optionally filtered by status.
pub async fn list_requests(
    pool: &TenantPool,
    tenant_id: TenantId,
    employee_id: Uuid,
    status: Option<&str>,
) -> Result<Vec<LeaveRequestView>, LeaveError> {
    let mut tx = pool.begin_tenant(tenant_id).await?;
    let rows = sqlx::query(
        r#"SELECT id, employee_id, leave_type, start_date, end_date, days, status, reason
           FROM leave_requests
           WHERE tenant_id = $1 AND employee_id = $2
             AND ($3::text IS NULL OR status = $3)
           ORDER BY start_date DESC
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
            Ok(LeaveRequestView {
                id: row.try_get("id")?,
                employee_id: row.try_get("employee_id")?,
                leave_type: row.try_get("leave_type")?,
                start_date: row.try_get("start_date")?,
                end_date: row.try_get("end_date")?,
                days: row.try_get("days")?,
                status: row.try_get("status")?,
                reason: row.try_get("reason")?,
            })
        })
        .collect::<Result<Vec<_>, sqlx::Error>>()
        .map_err(Into::into)
}

/// Pending leave request for an HR/manager inbox.
#[derive(Debug, Serialize)]
pub struct PendingLeave {
    pub id: Uuid,
    pub employee_id: Uuid,
    pub employee_code: String,
    pub employee_name: String,
    pub leave_type: String,
    pub label_th: String,
    pub start_date: NaiveDate,
    pub end_date: NaiveDate,
    pub days: Decimal,
    pub reason: Option<String>,
    pub step_no: i32,
    pub total_steps: i64,
    pub approver_role: String,
}

/// Cancel a leave request. Only the owning employee may cancel, and only
/// requests in `pending` status.
pub async fn cancel_request(
    pool: &TenantPool,
    tenant_id: TenantId,
    user_id: Uuid,
    request_id: Uuid,
) -> Result<String, LeaveError> {
    let employee_id = match employee_of_user(pool, tenant_id, user_id).await? {
        Some(id) => id,
        None => return Err(LeaveError::NotFound),
    };

    let mut tx = pool.begin_tenant(tenant_id).await?;

    // Optimistic update guarded by status and ownership.
    let updated = sqlx::query(
        r#"UPDATE leave_requests
           SET status = 'cancelled', updated_at = now()
           WHERE id = $1 AND tenant_id = $2 AND employee_id = $3 AND status = 'pending'
           RETURNING status, leave_type, start_date, days, balance_debited"#,
    )
    .bind(request_id)
    .bind(tenant_id.0)
    .bind(employee_id)
    .fetch_optional(&mut *tx)
    .await?;

    if let Some(row) = updated {
        let status: String = row.try_get("status")?;
        let leave_type: String = row.try_get("leave_type")?;
        let year: i32 = row.try_get::<NaiveDate, _>("start_date")?.year();
        let days: Decimal = row.try_get("days")?;
        let was_debited: bool = row.try_get("balance_debited")?;

        sqlx::query(
            "UPDATE leave_approval_steps SET status = 'skipped' WHERE leave_request_id = $1 AND status = 'pending'",
        )
        .bind(request_id)
        .execute(&mut *tx)
        .await?;

        if was_debited && leave_type != "unpaid" {
            sqlx::query(
                r#"UPDATE leave_balances lb
                   SET used_days = used_days - $1, updated_at = now()
                   FROM leave_types lt
                   WHERE lb.employee_id = $2
                     AND lb.year = $3
                     AND lt.id = lb.leave_type_id
                     AND lt.name = $4
                     AND (lt.tenant_id IS NULL OR lt.tenant_id = $5)"#,
            )
            .bind(days)
            .bind(employee_id)
            .bind(year)
            .bind(&leave_type)
            .bind(tenant_id.0)
            .execute(&mut *tx)
            .await?;
        }

        tx.commit().await?;
        return Ok(status);
    }

    // No row updated: distinguish not-found / not-own from wrong status.
    let row = sqlx::query(
        r#"SELECT status, employee_id FROM leave_requests
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
                Err(LeaveError::NotFound)
            } else {
                Err(LeaveError::NotCancellable)
            }
        }
        None => Err(LeaveError::NotFound),
    }
}

/// Approve or reject a pending leave request. Rejection restores the balance
/// debit when `balance_debited` is true; approval keeps it.
pub async fn decide_request(
    pool: &TenantPool,
    tenant_id: TenantId,
    approver_user_id: Uuid,
    roles: &[String],
    request_id: Uuid,
    approve: bool,
) -> Result<DecideOutcome, LeaveError> {
    let mut tx = pool.begin_tenant(tenant_id).await?;

    let request = sqlx::query(
        r#"SELECT id, employee_id, leave_type, start_date, end_date, days, balance_debited
           FROM leave_requests
           WHERE id = $1 AND tenant_id = $2 AND status = 'pending'
           FOR UPDATE"#,
    )
    .bind(request_id)
    .bind(tenant_id.0)
    .fetch_optional(&mut *tx)
    .await?;

    let Some(request) = request else {
        let _ = tx.rollback().await;
        return Err(LeaveError::NotFound);
    };

    let employee_id: Uuid = request.try_get("employee_id")?;
    let leave_type: String = request.try_get("leave_type")?;
    let start_date: NaiveDate = request.try_get("start_date")?;
    let end_date: NaiveDate = request.try_get("end_date")?;
    let days: Decimal = request.try_get("days")?;

    // Find the current pending step, creating a single HR step on the fly for
    // legacy requests that pre-date the steps table.
    let step = sqlx::query(
        r#"SELECT id, step_no, approver_role
           FROM leave_approval_steps
           WHERE leave_request_id = $1 AND status = 'pending'
           ORDER BY step_no
           LIMIT 1
           FOR UPDATE"#,
    )
    .bind(request_id)
    .fetch_optional(&mut *tx)
    .await?;

    let (step_id, step_no, approver_role) = match step {
        Some(row) => {
            let id: Uuid = row.try_get("id")?;
            let no: i32 = row.try_get("step_no")?;
            let role: String = row.try_get("approver_role")?;
            (id, no, role)
        }
        None => {
            let id = Uuid::new_v4();
            sqlx::query(
                r#"INSERT INTO leave_approval_steps
                     (id, tenant_id, leave_request_id, step_no, approver_role, status)
                   VALUES ($1,$2,$3,1,'hr_admin','pending')"#,
            )
            .bind(id)
            .bind(tenant_id.0)
            .bind(request_id)
            .execute(&mut *tx)
            .await?;
            (id, 1, "hr_admin".to_string())
        }
    };

    let can_decide = roles.iter().any(|r| r == &approver_role || r == "hr_admin");
    if !can_decide {
        let _ = tx.rollback().await;
        return Err(LeaveError::Forbidden);
    }

    let total_steps: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM leave_approval_steps WHERE leave_request_id = $1")
            .bind(request_id)
            .fetch_one(&mut *tx)
            .await?;

    let final_status: &'static str;
    let outcome_step: i32;

    if approve {
        sqlx::query(
            r#"UPDATE leave_approval_steps
               SET status = 'approved', decided_by = $1, decided_at = now()
               WHERE id = $2"#,
        )
        .bind(approver_user_id)
        .bind(step_id)
        .execute(&mut *tx)
        .await?;

        let next_step: Option<i32> = sqlx::query_scalar(
            r#"SELECT step_no FROM leave_approval_steps
               WHERE leave_request_id = $1 AND status = 'pending'
               ORDER BY step_no
               LIMIT 1"#,
        )
        .bind(request_id)
        .fetch_optional(&mut *tx)
        .await?;

        if let Some(next) = next_step {
            tx.commit().await?;
            return Ok(DecideOutcome {
                status: "pending".to_string(),
                step: next,
                total_steps,
            });
        }

        sqlx::query(
            r#"UPDATE leave_requests
               SET status = 'approved', approver_id = $1, approved_at = now(), updated_at = now()
               WHERE id = $2"#,
        )
        .bind(approver_user_id)
        .bind(request_id)
        .execute(&mut *tx)
        .await?;

        final_status = "approved";
        outcome_step = total_steps as i32;
    } else {
        sqlx::query(
            r#"UPDATE leave_approval_steps
               SET status = 'rejected', decided_by = $1, decided_at = now()
               WHERE id = $2"#,
        )
        .bind(approver_user_id)
        .bind(step_id)
        .execute(&mut *tx)
        .await?;

        sqlx::query(
            r#"UPDATE leave_approval_steps
               SET status = 'skipped'
               WHERE leave_request_id = $1 AND status = 'pending'"#,
        )
        .bind(request_id)
        .execute(&mut *tx)
        .await?;

        sqlx::query(
            r#"UPDATE leave_requests
               SET status = 'rejected', approver_id = $1, approved_at = now(), updated_at = now()
               WHERE id = $2"#,
        )
        .bind(approver_user_id)
        .bind(request_id)
        .execute(&mut *tx)
        .await?;

        let was_debited: bool = request.try_get("balance_debited")?;
        if was_debited && leave_type != "unpaid" {
            let year: i32 = start_date.year();
            sqlx::query(
                r#"UPDATE leave_balances lb
                   SET used_days = used_days - $1, updated_at = now()
                   FROM leave_types lt
                   WHERE lb.employee_id = $2
                     AND lb.year = $3
                     AND lt.id = lb.leave_type_id
                     AND lt.name = $4
                     AND (lt.tenant_id IS NULL OR lt.tenant_id = $5)"#,
            )
            .bind(days)
            .bind(employee_id)
            .bind(year)
            .bind(&leave_type)
            .bind(tenant_id.0)
            .execute(&mut *tx)
            .await?;

            sqlx::query(
                "UPDATE leave_requests SET balance_debited = FALSE WHERE id = $1 AND tenant_id = $2",
            )
            .bind(request_id)
            .bind(tenant_id.0)
            .execute(&mut *tx)
            .await?;
        }

        final_status = "rejected";
        outcome_step = step_no;
    }

    let label_th: String = sqlx::query_scalar(
        "SELECT label_th FROM leave_types WHERE name = $1 AND (tenant_id IS NULL OR tenant_id = $2) ORDER BY (tenant_id IS NOT NULL) DESC, label_th LIMIT 1"
    )
    .bind(&leave_type)
    .bind(tenant_id.0)
    .fetch_optional(&mut *tx)
    .await?
    .unwrap_or_else(|| leave_type.clone());

    enqueue_leave_decided(
        &mut tx,
        tenant_id.0,
        employee_id,
        final_status,
        &label_th,
        start_date,
        end_date,
    )
    .await?;

    tx.commit().await?;
    Ok(DecideOutcome {
        status: final_status.to_string(),
        step: outcome_step,
        total_steps,
    })
}

/// All pending leave requests across the tenant, newest first.
pub async fn pending_approvals(
    pool: &TenantPool,
    tenant_id: TenantId,
) -> Result<Vec<PendingLeave>, LeaveError> {
    let mut tx = pool.begin_tenant(tenant_id).await?;
    let rows = sqlx::query(
        r#"SELECT lr.id, lr.employee_id, e.employee_code, e.first_name_th, e.last_name_th,
                  lr.leave_type, lt.label_th, lr.start_date, lr.end_date, lr.days, lr.reason,
                  COALESCE(
                      (SELECT step_no FROM leave_approval_steps
                        WHERE leave_request_id = lr.id AND status = 'pending'
                        ORDER BY step_no LIMIT 1),
                      1
                  ) AS step_no,
                  COALESCE(
                      (SELECT approver_role FROM leave_approval_steps
                        WHERE leave_request_id = lr.id AND status = 'pending'
                        ORDER BY step_no LIMIT 1),
                      'hr_admin'
                  ) AS approver_role,
                  GREATEST(
                      1,
                      (SELECT COUNT(*) FROM leave_approval_steps s2 WHERE s2.leave_request_id = lr.id)
                  ) AS total_steps
           FROM leave_requests lr
           JOIN employees e ON e.id = lr.employee_id
           JOIN leave_types lt ON lt.name = lr.leave_type
                               AND (lt.tenant_id IS NULL OR lt.tenant_id = lr.tenant_id)
           WHERE lr.tenant_id = $1 AND lr.status = 'pending'
           ORDER BY lr.created_at DESC
           LIMIT 200"#,
    )
    .bind(tenant_id.0)
    .fetch_all(&mut *tx)
    .await?;
    tx.commit().await?;

    rows.into_iter()
        .map(|row| {
            Ok(PendingLeave {
                id: row.try_get("id")?,
                employee_id: row.try_get("employee_id")?,
                employee_code: row.try_get("employee_code")?,
                employee_name: format!(
                    "{} {}",
                    row.try_get::<String, _>("first_name_th")?,
                    row.try_get::<String, _>("last_name_th")?
                ),
                leave_type: row.try_get("leave_type")?,
                label_th: row.try_get("label_th")?,
                start_date: row.try_get("start_date")?,
                end_date: row.try_get("end_date")?,
                days: row.try_get("days")?,
                reason: row.try_get("reason")?,
                step_no: row.try_get("step_no")?,
                total_steps: row.try_get("total_steps")?,
                approver_role: row.try_get("approver_role")?,
            })
        })
        .collect::<Result<Vec<_>, sqlx::Error>>()
        .map_err(Into::into)
}

/// Employee record owned by a login user (self-service default).
pub async fn employee_of_user(
    pool: &TenantPool,
    tenant_id: TenantId,
    user_id: Uuid,
) -> Result<Option<Uuid>, LeaveError> {
    let mut tx = pool.begin_tenant(tenant_id).await?;
    let row = sqlx::query("SELECT id FROM employees WHERE tenant_id = $1 AND user_id = $2 LIMIT 1")
        .bind(tenant_id.0)
        .bind(user_id)
        .fetch_optional(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(row.map(|r| r.get("id")))
}

/// One row of an opening leave-balance import.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct BalanceImportRow {
    pub employee_code: String,
    pub leave_type: String,
    pub year: i32,
    pub total_days: Decimal,
    pub used_days: Option<Decimal>,
}

/// Per-row result for an import report.
#[derive(Debug, Clone, Serialize)]
pub struct BalanceImportRowResult {
    pub row: usize,
    pub employee_code: String,
    pub leave_type: String,
    pub year: i32,
    pub status: String,
    pub errors: Vec<String>,
}

/// Report returned by [`import_balances`].
#[derive(Debug, Serialize)]
pub struct BalanceImportReport {
    pub dry_run: bool,
    pub total: usize,
    pub valid: usize,
    pub invalid: usize,
    pub created: usize,
    pub rows: Vec<BalanceImportRowResult>,
}

/// Import (or dry-run) opening leave balances for a tenant.
///
/// Employee codes and leave types are resolved inside the tenant; global
/// `tenant_id IS NULL` leave types are allowed. The whole file is all-or-nothing:
/// any invalid row causes `created: 0` and leaves the database untouched.
pub async fn import_balances(
    pool: &TenantPool,
    tenant_id: TenantId,
    dry_run: bool,
    rows: Vec<BalanceImportRow>,
) -> Result<BalanceImportReport, LeaveError> {
    let total = rows.len();

    // Resolve employees and leave types in a single round-trip each.
    let mut resolve_tx = pool.begin_tenant(tenant_id).await?;

    let mut employee_map: HashMap<String, Uuid> = HashMap::new();
    let codes: Vec<String> = rows
        .iter()
        .map(|r| r.employee_code.trim().to_string())
        .filter(|c| !c.is_empty())
        .collect::<std::collections::HashSet<_>>()
        .into_iter()
        .collect();
    if !codes.is_empty() {
        let employees = sqlx::query(
            "SELECT id, employee_code FROM employees WHERE tenant_id = $1 AND employee_code = ANY($2)",
        )
        .bind(tenant_id.0)
        .bind(&codes)
        .fetch_all(&mut *resolve_tx)
        .await?;
        for row in employees {
            let code: String = row.try_get("employee_code")?;
            let id: Uuid = row.try_get("id")?;
            employee_map.insert(code, id);
        }
    }

    let mut leave_type_map: HashMap<String, i32> = HashMap::new();
    let type_names: Vec<String> = rows
        .iter()
        .map(|r| r.leave_type.trim().to_string())
        .filter(|t| !t.is_empty())
        .collect::<std::collections::HashSet<_>>()
        .into_iter()
        .collect();
    if !type_names.is_empty() {
        let types = sqlx::query(
            "SELECT id, name, tenant_id FROM leave_types WHERE name = ANY($1) AND (tenant_id IS NULL OR tenant_id = $2)",
        )
        .bind(&type_names)
        .bind(tenant_id.0)
        .fetch_all(&mut *resolve_tx)
        .await?;
        // Prefer tenant-specific types over global defaults.
        let mut chosen: HashMap<String, (bool, i32)> = HashMap::new();
        for row in types {
            let id: i32 = row.try_get("id")?;
            let name: String = row.try_get("name")?;
            let is_tenant: bool = row.try_get::<Option<Uuid>, _>("tenant_id")?.is_some();
            let entry = chosen.entry(name).or_insert((false, id));
            if is_tenant {
                entry.0 = true;
                entry.1 = id;
            }
        }
        for (name, (_, id)) in chosen {
            leave_type_map.insert(name, id);
        }
    }

    let _ = resolve_tx.rollback().await;

    // Detect duplicates by (employee_code, leave_type, year) inside the file.
    let mut occurrences: HashMap<(String, String, i32), Vec<usize>> = HashMap::new();
    for (idx, row) in rows.iter().enumerate() {
        let code = row.employee_code.trim();
        let typ = row.leave_type.trim();
        if !code.is_empty() && !typ.is_empty() {
            occurrences
                .entry((code.to_string(), typ.to_string(), row.year))
                .or_default()
                .push(idx);
        }
    }
    let duplicates: std::collections::HashSet<(String, String, i32)> = occurrences
        .iter()
        .filter(|(_, indices)| indices.len() > 1)
        .map(|(key, _)| key.clone())
        .collect();

    let mut row_results = Vec::with_capacity(total);
    let mut parsed: Vec<Option<(Uuid, i32, Decimal, Decimal)>> = Vec::with_capacity(total);
    let mut invalid = 0usize;

    for (idx, row) in rows.iter().enumerate() {
        let mut errors = Vec::new();
        let code = row.employee_code.trim();
        let typ = row.leave_type.trim();

        if code.is_empty() {
            errors.push("employee_code is required".into());
        }
        if typ.is_empty() {
            errors.push("leave_type is required".into());
        }
        if !(2000..=2100).contains(&row.year) {
            errors.push("year must be between 2000 and 2100".into());
        }
        if row.total_days < Decimal::ZERO {
            errors.push("total_days must be >= 0".into());
        }
        if row.total_days > Decimal::from(366) {
            errors.push("total_days must be <= 366".into());
        }
        let used = row.used_days.unwrap_or(Decimal::ZERO);
        if used < Decimal::ZERO {
            errors.push("used_days must be >= 0".into());
        }
        if used > row.total_days {
            errors.push("used_days must not exceed total_days".into());
        }
        if duplicates.contains(&(code.to_string(), typ.to_string(), row.year)) {
            errors.push(format!(
                "duplicate row for ({}, {}, {}) in file",
                code, typ, row.year
            ));
        }
        if !code.is_empty() && !employee_map.contains_key(code) {
            errors.push(format!("employee_code {} not found", code));
        }
        if !typ.is_empty() && !leave_type_map.contains_key(typ) {
            errors.push(format!("leave_type {} not found", typ));
        }

        let status = if errors.is_empty() {
            "valid"
        } else {
            invalid += 1;
            "invalid"
        };

        parsed.push(if errors.is_empty() {
            Some((
                employee_map[code],
                leave_type_map[typ],
                row.total_days,
                used,
            ))
        } else {
            None
        });

        row_results.push(BalanceImportRowResult {
            row: idx + 1,
            employee_code: row.employee_code.clone(),
            leave_type: row.leave_type.clone(),
            year: row.year,
            status: status.into(),
            errors,
        });
    }

    let valid = total - invalid;
    let created = if dry_run || invalid > 0 { 0 } else { valid };

    if !dry_run && invalid == 0 && valid > 0 {
        let mut tx = pool.begin_tenant(tenant_id).await?;
        for (idx, maybe) in parsed.iter().enumerate() {
            let Some((employee_id, leave_type_id, total_days, used_days)) = maybe else {
                continue;
            };
            sqlx::query(
                r#"INSERT INTO leave_balances
                     (tenant_id, employee_id, leave_type_id, year, total_days, used_days)
                   VALUES ($1, $2, $3, $4, $5, $6)
                   ON CONFLICT (employee_id, leave_type_id, year)
                   DO UPDATE SET total_days = EXCLUDED.total_days,
                                 used_days = EXCLUDED.used_days,
                                 updated_at = now()"#,
            )
            .bind(tenant_id.0)
            .bind(employee_id)
            .bind(leave_type_id)
            .bind(rows[idx].year)
            .bind(total_days)
            .bind(used_days)
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
    }

    Ok(BalanceImportReport {
        dry_run,
        total,
        valid,
        invalid,
        created,
        rows: row_results,
    })
}

/// Coerce a missing or invalid chain to the safe one-step HR default.
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

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).unwrap()
    }

    #[test]
    fn single_day_request_counts_as_one() {
        assert_eq!(
            count_leave_days(d(2026, 10, 5), d(2026, 10, 5)).unwrap(),
            dec!(1)
        );
    }

    #[test]
    fn range_is_inclusive() {
        assert_eq!(
            count_leave_days(d(2026, 10, 1), d(2026, 10, 3)).unwrap(),
            dec!(3)
        );
    }

    #[test]
    fn reversed_range_is_rejected() {
        assert!(matches!(
            count_leave_days(d(2026, 10, 3), d(2026, 10, 1)),
            Err(LeaveError::BadRange)
        ));
    }

    #[test]
    fn over_a_year_is_rejected() {
        assert!(matches!(
            count_leave_days(d(2026, 1, 1), d(2027, 6, 1)),
            Err(LeaveError::TooLong)
        ));
    }

    #[test]
    fn leave_type_whitelist_matches_statutory_set() {
        assert_eq!(LEAVE_TYPES.len(), 7);
        assert!(LEAVE_TYPES.contains(&"maternity"));
        assert!(!LEAVE_TYPES.contains(&"holiday"));
    }

    #[test]
    fn normalise_chain_defaults_invalid_chains() {
        assert_eq!(normalise_chain(vec![]), vec!["hr_admin"]);
        assert_eq!(normalise_chain(vec!["manager".into()]), vec!["hr_admin"]);
        assert_eq!(
            normalise_chain(vec!["manager".into(), "hr_admin".into()]),
            vec!["manager", "hr_admin"]
        );
        assert_eq!(
            normalise_chain(vec![
                "manager".into(),
                "manager".into(),
                "hr_admin".into(),
                "hr_admin".into()
            ]),
            vec!["hr_admin"]
        );
        assert_eq!(
            normalise_chain(vec!["manager".into(), "manager".into(), "hr_admin".into()]),
            vec!["hr_admin"],
            "duplicate roles should normalise to default"
        );
    }
}
