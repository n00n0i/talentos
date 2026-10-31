//! Performance goals/OKRs per employee, optionally tied to a review cycle.
//! Tenant isolation is `begin_tenant` + explicit `tenant_id` predicates.

use chrono::{DateTime, Utc};
use serde::Serialize;
use sqlx::Row;
use talentos_db::TenantPool;
use talentos_kernel::TenantId;
use uuid::Uuid;

const GOAL_STATUSES: &[&str] = &["active", "achieved", "cancelled"];

#[derive(Debug, thiserror::Error)]
pub enum GoalError {
    #[error("employee not found for this tenant")]
    EmployeeNotFound,
    #[error("review cycle not found for this tenant")]
    CycleNotFound,
    #[error("goal not found")]
    GoalNotFound,
    #[error("title must be 1-200 characters")]
    InvalidTitle,
    #[error("description must be at most 1000 characters")]
    InvalidDescription,
    #[error("target must be at most 200 characters")]
    InvalidTarget,
    #[error("progress must be between 0 and 100")]
    InvalidProgress,
    #[error("unsupported goal status")]
    InvalidStatus,
    #[error("database error: {0}")]
    Db(#[from] sqlx::Error),
}

#[derive(Debug, Clone)]
pub struct GoalInput {
    pub cycle_id: Option<Uuid>,
    pub title: String,
    pub description: Option<String>,
    pub target: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct GoalView {
    pub id: Uuid,
    pub employee_id: Uuid,
    pub cycle_id: Option<Uuid>,
    pub cycle_name: Option<String>,
    pub title: String,
    pub description: Option<String>,
    pub target: Option<String>,
    pub progress: i32,
    pub status: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

const GOAL_SELECT: &str = r#"SELECT g.id, g.employee_id, g.cycle_id, c.name AS cycle_name,
              g.title, g.description, g.target, g.progress, g.status,
              g.created_at, g.updated_at
         FROM performance_goals g
         LEFT JOIN performance_cycles c
                ON c.id = g.cycle_id AND c.tenant_id = g.tenant_id
        WHERE g.tenant_id = $1"#;

fn map_goal(row: &sqlx::postgres::PgRow) -> Result<GoalView, sqlx::Error> {
    Ok(GoalView {
        id: row.try_get("id")?,
        employee_id: row.try_get("employee_id")?,
        cycle_id: row.try_get("cycle_id")?,
        cycle_name: row.try_get("cycle_name")?,
        title: row.try_get("title")?,
        description: row.try_get("description")?,
        target: row.try_get("target")?,
        progress: row.try_get("progress")?,
        status: row.try_get("status")?,
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
    })
}

pub async fn list_goals(
    pool: &TenantPool,
    tenant_id: TenantId,
    employee_id: Uuid,
    cycle_id: Option<Uuid>,
) -> Result<Vec<GoalView>, GoalError> {
    let mut tx = pool.begin_tenant(tenant_id).await?;
    let sql = format!("{GOAL_SELECT} AND g.employee_id = $2 AND ($3::uuid IS NULL OR g.cycle_id = $3) ORDER BY (g.status = 'active') DESC, g.created_at DESC");
    let rows = sqlx::query(&sql)
        .bind(tenant_id.0)
        .bind(employee_id)
        .bind(cycle_id)
        .fetch_all(&mut *tx)
        .await?;
    tx.commit().await?;
    rows.iter()
        .map(|row| map_goal(row).map_err(GoalError::from))
        .collect()
}

pub async fn create_goal(
    pool: &TenantPool,
    tenant_id: TenantId,
    user_id: Uuid,
    employee_id: Uuid,
    input: &GoalInput,
) -> Result<GoalView, GoalError> {
    let title = input.title.trim();
    if title.is_empty() || title.chars().count() > 200 {
        return Err(GoalError::InvalidTitle);
    }
    let description = input
        .description
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty());
    if description.is_some_and(|v| v.chars().count() > 1000) {
        return Err(GoalError::InvalidDescription);
    }
    let target = input
        .target
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty());
    if target.is_some_and(|v| v.chars().count() > 200) {
        return Err(GoalError::InvalidTarget);
    }

    let mut tx = pool.begin_tenant(tenant_id).await?;
    let employee: Option<Uuid> =
        sqlx::query_scalar("SELECT id FROM employees WHERE tenant_id = $1 AND id = $2")
            .bind(tenant_id.0)
            .bind(employee_id)
            .fetch_optional(&mut *tx)
            .await?;
    if employee.is_none() {
        return Err(GoalError::EmployeeNotFound);
    }
    if let Some(cycle_id) = input.cycle_id {
        let cycle: Option<Uuid> = sqlx::query_scalar(
            "SELECT id FROM performance_cycles WHERE tenant_id = $1 AND id = $2",
        )
        .bind(tenant_id.0)
        .bind(cycle_id)
        .fetch_optional(&mut *tx)
        .await?;
        if cycle.is_none() {
            return Err(GoalError::CycleNotFound);
        }
    }

    let row = sqlx::query(
        r#"INSERT INTO performance_goals
             (tenant_id, employee_id, cycle_id, title, description, target, created_by)
           VALUES ($1,$2,$3,$4,$5,$6,$7)
           RETURNING id, employee_id, cycle_id, NULL::text AS cycle_name, title, description,
                     target, progress, status, created_at, updated_at"#,
    )
    .bind(tenant_id.0)
    .bind(employee_id)
    .bind(input.cycle_id)
    .bind(title)
    .bind(description)
    .bind(target)
    .bind(user_id)
    .fetch_one(&mut *tx)
    .await?;
    let mut view = map_goal(&row)?;
    if let Some(cycle_id) = input.cycle_id {
        view.cycle_name = sqlx::query_scalar(
            "SELECT name FROM performance_cycles WHERE tenant_id = $1 AND id = $2",
        )
        .bind(tenant_id.0)
        .bind(cycle_id)
        .fetch_optional(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    Ok(view)
}

pub async fn update_goal(
    pool: &TenantPool,
    tenant_id: TenantId,
    goal_id: Uuid,
    progress: i32,
    status: &str,
) -> Result<GoalView, GoalError> {
    if !(0..=100).contains(&progress) {
        return Err(GoalError::InvalidProgress);
    }
    if !GOAL_STATUSES.contains(&status) {
        return Err(GoalError::InvalidStatus);
    }
    let mut tx = pool.begin_tenant(tenant_id).await?;
    sqlx::query(
        "UPDATE performance_goals SET progress = $3, status = $4, updated_at = now()
          WHERE tenant_id = $1 AND id = $2",
    )
    .bind(tenant_id.0)
    .bind(goal_id)
    .bind(progress)
    .bind(status)
    .execute(&mut *tx)
    .await?;

    let sql = format!("{GOAL_SELECT} AND g.id = $2");
    let row = sqlx::query(&sql)
        .bind(tenant_id.0)
        .bind(goal_id)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or(GoalError::GoalNotFound)?;
    let view = map_goal(&row)?;
    tx.commit().await?;
    Ok(view)
}

pub async fn delete_goal(
    pool: &TenantPool,
    tenant_id: TenantId,
    goal_id: Uuid,
) -> Result<(), GoalError> {
    let mut tx = pool.begin_tenant(tenant_id).await?;
    let deleted = sqlx::query("DELETE FROM performance_goals WHERE tenant_id = $1 AND id = $2")
        .bind(tenant_id.0)
        .bind(goal_id)
        .execute(&mut *tx)
        .await?;
    if deleted.rows_affected() == 0 {
        return Err(GoalError::GoalNotFound);
    }
    tx.commit().await?;
    Ok(())
}

pub async fn list_my_goals(
    pool: &TenantPool,
    tenant_id: TenantId,
    user_id: Uuid,
) -> Result<Vec<GoalView>, GoalError> {
    let mut tx = pool.begin_tenant(tenant_id).await?;
    let sql = format!("{GOAL_SELECT} AND g.employee_id IN (SELECT e.id FROM employees e WHERE e.tenant_id = $1 AND e.user_id = $2) ORDER BY (g.status = 'active') DESC, g.created_at DESC");
    let rows = sqlx::query(&sql)
        .bind(tenant_id.0)
        .bind(user_id)
        .fetch_all(&mut *tx)
        .await?;
    tx.commit().await?;
    rows.iter()
        .map(|row| map_goal(row).map_err(GoalError::from))
        .collect()
}
