//! Approval delegation: a manager/HR can hand leave-approval authority to an
//! active tenant user for a date range. `decide_request` consults this table
//! when the caller lacks the step role.

use chrono::NaiveDate;
use serde::{Deserialize, Serialize};
use sqlx::Row;
use talentos_db::TenantPool;
use talentos_kernel::TenantId;
use uuid::Uuid;

#[derive(Debug, thiserror::Error)]
pub enum DelegationError {
    #[error("delegate must be an active user in this tenant")]
    InvalidDelegate,
    #[error("delegator must be an active user in this tenant")]
    InvalidDelegator,
    #[error("you cannot delegate approval to yourself")]
    SelfDelegation,
    #[error("ends_on must be on or after starts_on")]
    BadRange,
    #[error("delegation not found")]
    NotFound,
    #[error("an identical delegation already exists")]
    Duplicate,
    #[error("database error: {0}")]
    Db(#[from] sqlx::Error),
}

#[derive(Debug, Serialize)]
pub struct DelegationView {
    pub id: Uuid,
    pub delegator_user_id: Uuid,
    pub delegate_user_id: Uuid,
    pub delegate_email: String,
    pub delegate_name: Option<String>,
    pub starts_on: NaiveDate,
    pub ends_on: NaiveDate,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DelegationInput {
    pub delegate_user_id: Uuid,
    pub starts_on: NaiveDate,
    pub ends_on: NaiveDate,
    /// Only an HR admin may name a delegator other than themselves.
    pub delegator_user_id: Option<Uuid>,
}

#[derive(Debug, Serialize)]
pub struct DelegateCandidate {
    pub id: Uuid,
    pub email: String,
    pub full_name_th: Option<String>,
}

const DELEGATION_COLS: &str = "d.id, d.delegator_user_id, d.delegate_user_id, u.email, u.full_name_th, d.starts_on, d.ends_on";

fn row_to_view(row: &sqlx::postgres::PgRow) -> Result<DelegationView, DelegationError> {
    Ok(DelegationView {
        id: row.try_get("id")?,
        delegator_user_id: row.try_get("delegator_user_id")?,
        delegate_user_id: row.try_get("delegate_user_id")?,
        delegate_email: row.try_get("email")?,
        delegate_name: row.try_get("full_name_th")?,
        starts_on: row.try_get("starts_on")?,
        ends_on: row.try_get("ends_on")?,
    })
}

/// Own delegations (an HR admin sees every delegation in the tenant).
pub async fn list_delegations(
    pool: &TenantPool,
    tenant_id: TenantId,
    caller_user_id: Uuid,
    is_hr: bool,
) -> Result<Vec<DelegationView>, DelegationError> {
    let mut tx = pool.begin_tenant(tenant_id).await?;
    let rows = sqlx::query(&format!(
        r#"SELECT {DELEGATION_COLS}
             FROM approval_delegations d
             JOIN users u ON u.id = d.delegate_user_id
            WHERE d.tenant_id = $1
              AND ($2::uuid IS NULL OR d.delegator_user_id = $2)
            ORDER BY d.created_at DESC"#
    ))
    .bind(tenant_id.0)
    .bind(if is_hr { None } else { Some(caller_user_id) })
    .fetch_all(&mut *tx)
    .await?;
    tx.commit().await?;
    rows.iter().map(row_to_view).collect()
}

/// Active tenant users a manager/HR may delegate to.
pub async fn list_candidates(
    pool: &TenantPool,
    tenant_id: TenantId,
) -> Result<Vec<DelegateCandidate>, DelegationError> {
    let mut tx = pool.begin_tenant(tenant_id).await?;
    let rows = sqlx::query(
        r#"SELECT id, email, full_name_th
             FROM users
            WHERE tenant_id = $1 AND is_active
            ORDER BY email"#,
    )
    .bind(tenant_id.0)
    .fetch_all(&mut *tx)
    .await?;
    tx.commit().await?;
    rows.iter()
        .map(|row| {
            Ok(DelegateCandidate {
                id: row.try_get("id")?,
                email: row.try_get("email")?,
                full_name_th: row.try_get("full_name_th")?,
            })
        })
        .collect()
}

async fn active_user(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    tenant_id: Uuid,
    user_id: Uuid,
) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM users WHERE id = $1 AND tenant_id = $2 AND is_active)",
    )
    .bind(user_id)
    .bind(tenant_id)
    .fetch_one(&mut **tx)
    .await
}

/// Create a delegation. The delegator is the caller unless an HR admin names one.
pub async fn create_delegation(
    pool: &TenantPool,
    tenant_id: TenantId,
    caller_user_id: Uuid,
    is_hr: bool,
    input: DelegationInput,
) -> Result<DelegationView, DelegationError> {
    let delegator = match input.delegator_user_id {
        Some(id) if is_hr => id,
        _ => caller_user_id,
    };
    let delegate_user_id = input.delegate_user_id;
    let starts_on = input.starts_on;
    let ends_on = input.ends_on;
    if delegator == delegate_user_id {
        return Err(DelegationError::SelfDelegation);
    }
    if ends_on < starts_on {
        return Err(DelegationError::BadRange);
    }

    let mut tx = pool.begin_tenant(tenant_id).await?;
    if !active_user(&mut tx, tenant_id.0, delegate_user_id).await? {
        let _ = tx.rollback().await;
        return Err(DelegationError::InvalidDelegate);
    }
    if delegator != caller_user_id && !active_user(&mut tx, tenant_id.0, delegator).await? {
        let _ = tx.rollback().await;
        return Err(DelegationError::InvalidDelegator);
    }

    let row = sqlx::query(
        r#"INSERT INTO approval_delegations
             (tenant_id, delegator_user_id, delegate_user_id, starts_on, ends_on, created_by)
           VALUES ($1, $2, $3, $4, $5, $6)
           RETURNING id, delegator_user_id, delegate_user_id, starts_on, ends_on"#,
    )
    .bind(tenant_id.0)
    .bind(delegator)
    .bind(delegate_user_id)
    .bind(starts_on)
    .bind(ends_on)
    .bind(caller_user_id)
    .fetch_one(&mut *tx)
    .await
    .map_err(|e| {
        if is_unique_violation(&e) {
            DelegationError::Duplicate
        } else {
            DelegationError::Db(e)
        }
    })?;

    let (email, name): (String, Option<String>) =
        sqlx::query_as("SELECT email, full_name_th FROM users WHERE id = $1 AND tenant_id = $2")
            .bind(delegate_user_id)
            .bind(tenant_id.0)
            .fetch_one(&mut *tx)
            .await?;

    tx.commit().await?;
    Ok(DelegationView {
        id: row.try_get("id")?,
        delegator_user_id: row.try_get("delegator_user_id")?,
        delegate_user_id: row.try_get("delegate_user_id")?,
        delegate_email: email,
        delegate_name: name,
        starts_on: row.try_get("starts_on")?,
        ends_on: row.try_get("ends_on")?,
    })
}

/// Delete a delegation. Only the delegator may delete; an HR admin may delete any.
pub async fn delete_delegation(
    pool: &TenantPool,
    tenant_id: TenantId,
    caller_user_id: Uuid,
    is_hr: bool,
    id: Uuid,
) -> Result<(), DelegationError> {
    let mut tx = pool.begin_tenant(tenant_id).await?;
    let deleted = sqlx::query(
        r#"DELETE FROM approval_delegations
            WHERE id = $1 AND tenant_id = $2
              AND ($3::uuid IS NULL OR delegator_user_id = $3)"#,
    )
    .bind(id)
    .bind(tenant_id.0)
    .bind(if is_hr { None } else { Some(caller_user_id) })
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    if deleted.rows_affected() == 0 {
        return Err(DelegationError::NotFound);
    }
    Ok(())
}

fn is_unique_violation(error: &sqlx::Error) -> bool {
    error
        .as_database_error()
        .and_then(|db| db.code())
        .is_some_and(|code| code == "23505")
}
