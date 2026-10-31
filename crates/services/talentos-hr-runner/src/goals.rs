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

#[derive(Debug, thiserror::Error)]
pub enum CheckInError {
    #[error("goal not found")]
    GoalNotFound,
    #[error("caller is not the direct manager of the goal owner")]
    NotManager,
    #[error("check-in note must be 1-2000 characters")]
    InvalidNote,
    #[error("database error: {0}")]
    Db(#[from] sqlx::Error),
}

#[derive(Debug, Serialize)]
pub struct CheckInView {
    pub id: Uuid,
    pub goal_id: Uuid,
    pub employee_id: Uuid,
    pub manager_id: Option<Uuid>,
    pub note: String,
    pub created_at: DateTime<Utc>,
}

/// Check-in notes logged on a goal (by the goal owner's manager).
pub async fn list_goal_check_ins(
    pool: &TenantPool,
    tenant_id: TenantId,
    goal_id: Uuid,
) -> Result<Vec<CheckInView>, CheckInError> {
    let mut tx = pool.begin_tenant(tenant_id).await?;
    let goal: Option<Uuid> =
        sqlx::query_scalar("SELECT id FROM performance_goals WHERE tenant_id = $1 AND id = $2")
            .bind(tenant_id.0)
            .bind(goal_id)
            .fetch_optional(&mut *tx)
            .await?;
    if goal.is_none() {
        return Err(CheckInError::GoalNotFound);
    }
    let rows = sqlx::query(
        r#"SELECT id, goal_id, employee_id, manager_id, note, created_at
             FROM performance_goal_check_ins
            WHERE tenant_id = $1 AND goal_id = $2
            ORDER BY created_at DESC, id DESC"#,
    )
    .bind(tenant_id.0)
    .bind(goal_id)
    .fetch_all(&mut *tx)
    .await?;
    tx.commit().await?;
    rows.iter()
        .map(|row| {
            Ok(CheckInView {
                id: row.try_get("id")?,
                goal_id: row.try_get("goal_id")?,
                employee_id: row.try_get("employee_id")?,
                manager_id: row.try_get("manager_id")?,
                note: row.try_get("note")?,
                created_at: row.try_get("created_at")?,
            })
        })
        .collect()
}

/// Log a check-in on a goal. Only the goal owner's direct manager may.
pub async fn add_goal_check_in(
    pool: &TenantPool,
    tenant_id: TenantId,
    user_id: Uuid,
    goal_id: Uuid,
    note: &str,
) -> Result<CheckInView, CheckInError> {
    let note = note.trim();
    if note.is_empty() || note.chars().count() > 2000 {
        return Err(CheckInError::InvalidNote);
    }
    let mut tx = pool.begin_tenant(tenant_id).await?;
    let owner: Option<Uuid> = sqlx::query_scalar(
        "SELECT employee_id FROM performance_goals WHERE tenant_id = $1 AND id = $2",
    )
    .bind(tenant_id.0)
    .bind(goal_id)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or(CheckInError::GoalNotFound)?;
    let caller: Uuid =
        sqlx::query_scalar("SELECT id FROM employees WHERE tenant_id = $1 AND user_id = $2")
            .bind(tenant_id.0)
            .bind(user_id)
            .fetch_optional(&mut *tx)
            .await?
            .ok_or(CheckInError::NotManager)?;
    let allowed: Option<Uuid> =
        sqlx::query_scalar("SELECT manager_id FROM employees WHERE tenant_id = $1 AND id = $2")
            .bind(tenant_id.0)
            .bind(owner)
            .fetch_optional(&mut *tx)
            .await?;
    if allowed != Some(caller) {
        return Err(CheckInError::NotManager);
    }
    let row = sqlx::query(
        r#"INSERT INTO performance_goal_check_ins
             (tenant_id, goal_id, employee_id, manager_id, note)
            VALUES ($1, $2, $3, $4, $5)
            RETURNING id, goal_id, employee_id, manager_id, note, created_at"#,
    )
    .bind(tenant_id.0)
    .bind(goal_id)
    .bind(owner)
    .bind(caller)
    .bind(note)
    .fetch_one(&mut *tx)
    .await?;
    sqlx::query(
        r#"INSERT INTO audit_log
             (tenant_id, actor_type, actor_id, action, resource_type, resource_id, after_state)
           VALUES ($1, 'user', $2, 'performance.check_in', 'performance_goal', $3, $4)"#,
    )
    .bind(tenant_id.0)
    .bind(user_id.to_string())
    .bind(goal_id.to_string())
    .bind(serde_json::json!({ "note": note, "manager_id": caller }))
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(CheckInView {
        id: row.try_get("id")?,
        goal_id: row.try_get("goal_id")?,
        employee_id: row.try_get("employee_id")?,
        manager_id: row.try_get("manager_id")?,
        note: row.try_get("note")?,
        created_at: row.try_get("created_at")?,
    })
}

/// Goals owned by the caller's direct reports.
pub async fn list_goals_for_manager(
    pool: &TenantPool,
    tenant_id: TenantId,
    user_id: Uuid,
) -> Result<Vec<GoalView>, GoalError> {
    let mut tx = pool.begin_tenant(tenant_id).await?;
    let sql = format!(
        "{GOAL_SELECT}
         AND g.employee_id IN (
               SELECT e2.id FROM employees e1
               JOIN employees e2 ON e2.manager_id = e1.id AND e2.tenant_id = e1.tenant_id
              WHERE e1.tenant_id = $1 AND e1.user_id = $2)
         ORDER BY (g.status = 'active') DESC, g.created_at DESC"
    );
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
