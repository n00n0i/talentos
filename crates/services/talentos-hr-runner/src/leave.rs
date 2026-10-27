use chrono::{Datelike, NaiveDate};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use sqlx::Row;
use talentos_db::TenantPool;
use talentos_kernel::TenantId;
use uuid::Uuid;

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
    #[error("database error: {0}")]
    Db(#[from] sqlx::Error),
}

#[derive(Debug, Deserialize)]
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
    let employee = sqlx::query(
        r#"SELECT id, entity_id, status FROM employees
           WHERE tenant_id = $1
             AND ($2::uuid IS NOT NULL AND id = $2::uuid OR $2::uuid IS NULL AND user_id = $3)
           LIMIT 1"#,
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

    tx.commit().await?;

    Ok(LeaveRequestView {
        id: row.try_get("id")?,
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
                  COALESCE(lb.used_days, 0) AS used_days
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
    pub start_date: NaiveDate,
    pub end_date: NaiveDate,
    pub days: Decimal,
    pub reason: Option<String>,
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
    request_id: Uuid,
    approve: bool,
) -> Result<String, LeaveError> {
    let mut tx = pool.begin_tenant(tenant_id).await?;

    let updated = sqlx::query(
        r#"UPDATE leave_requests
           SET status = $3, approver_id = $4, approved_at = now(), updated_at = now()
           WHERE id = $1 AND tenant_id = $2 AND status = 'pending'
           RETURNING status, employee_id, leave_type, start_date, days, balance_debited"#,
    )
    .bind(request_id)
    .bind(tenant_id.0)
    .bind(if approve { "approved" } else { "rejected" })
    .bind(approver_user_id)
    .fetch_optional(&mut *tx)
    .await?;

    let Some(row) = updated else {
        let _ = tx.rollback().await;
        return Err(LeaveError::NotFound);
    };

    let status: String = row.try_get("status")?;

    if !approve {
        let was_debited: bool = row.try_get("balance_debited")?;
        if was_debited {
            let employee_id: Uuid = row.try_get("employee_id")?;
            let leave_type: String = row.try_get("leave_type")?;
            let year: i32 = row.try_get::<NaiveDate, _>("start_date")?.year();
            let days: Decimal = row.try_get("days")?;

            if leave_type != "unpaid" {
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
                    "UPDATE leave_requests SET balance_debited = FALSE WHERE id = $1 AND tenant_id = $2"
                )
                .bind(request_id)
                .bind(tenant_id.0)
                .execute(&mut *tx)
                .await?;
            }
        }
    }

    tx.commit().await?;
    Ok(status)
}

/// All pending leave requests across the tenant, newest first.
pub async fn pending_approvals(
    pool: &TenantPool,
    tenant_id: TenantId,
) -> Result<Vec<PendingLeave>, LeaveError> {
    let mut tx = pool.begin_tenant(tenant_id).await?;
    let rows = sqlx::query(
        r#"SELECT lr.id, lr.employee_id, e.employee_code, e.first_name_th, e.last_name_th,
                  lr.leave_type, lr.start_date, lr.end_date, lr.days, lr.reason
           FROM leave_requests lr
           JOIN employees e ON e.id = lr.employee_id
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
                start_date: row.try_get("start_date")?,
                end_date: row.try_get("end_date")?,
                days: row.try_get("days")?,
                reason: row.try_get("reason")?,
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
}
