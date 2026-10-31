//! Employee onboarding checklist: standard tasks from the hire date, ad-hoc
//! tasks, completion tracking, and employee self-service.

use chrono::{DateTime, Duration, NaiveDate, Utc};
use serde::Serialize;
use sqlx::Row;
use talentos_attendance::bangkok_offset;
use talentos_db::TenantPool;
use talentos_kernel::TenantId;
use uuid::Uuid;

/// Standard checklist: title and days after the hire date.
const STANDARD_TASKS: &[(&str, i64)] = &[
    ("ลงนามสัญญาจ้าง", 0),
    ("ส่งสำเนาบัตรประชาชน", 3),
    ("แจ้งเลขบัญชีเงินเดือน", 5),
    ("ส่งเอกสารประกันสังคม", 7),
    ("อบรมความปลอดภัยและกฎบริษัท", 7),
    ("รับอุปกรณ์และตั้งค่าอีเมล", 1),
];

#[derive(Debug, thiserror::Error)]
pub enum OnboardingError {
    #[error("employee not found for this tenant")]
    EmployeeNotFound,
    #[error("onboarding task not found")]
    NotFound,
    #[error("an onboarding task with this title already exists")]
    DuplicateTask,
    #[error("title must be 1-200 characters")]
    InvalidTitle,
    #[error("notes must be at most 500 characters")]
    InvalidNotes,
    #[error("database error: {0}")]
    Db(#[from] sqlx::Error),
}

#[derive(Debug, Clone)]
pub struct TaskInput {
    pub title: String,
    pub notes: Option<String>,
    pub due_date: Option<NaiveDate>,
}

#[derive(Debug, Serialize)]
pub struct TaskView {
    pub id: Uuid,
    pub employee_id: Uuid,
    pub title: String,
    pub notes: Option<String>,
    pub due_date: Option<NaiveDate>,
    pub status: String,
    pub completed_at: Option<DateTime<Utc>>,
    /// Days until the due date; negative when overdue. `None` when no due date.
    pub days_left: Option<i64>,
}

fn today() -> NaiveDate {
    Utc::now().with_timezone(&bangkok_offset()).date_naive()
}

fn map_task(row: &sqlx::postgres::PgRow, today: NaiveDate) -> Result<TaskView, sqlx::Error> {
    let due_date: Option<NaiveDate> = row.try_get("due_date")?;
    Ok(TaskView {
        id: row.try_get("id")?,
        employee_id: row.try_get("employee_id")?,
        title: row.try_get("title")?,
        notes: row.try_get("notes")?,
        due_date,
        status: row.try_get("status")?,
        completed_at: row.try_get("completed_at")?,
        days_left: due_date.map(|d| (d - today).num_days()),
    })
}

pub async fn list_tasks(
    pool: &TenantPool,
    tenant_id: TenantId,
    employee_id: Uuid,
) -> Result<Vec<TaskView>, OnboardingError> {
    let mut tx = pool.begin_tenant(tenant_id).await?;
    let rows = sqlx::query(
        r#"SELECT id, employee_id, title, notes, due_date, status, completed_at
             FROM employee_onboarding_tasks
            WHERE tenant_id = $1 AND employee_id = $2
            ORDER BY (status = 'done'), due_date NULLS LAST, created_at"#,
    )
    .bind(tenant_id.0)
    .bind(employee_id)
    .fetch_all(&mut *tx)
    .await?;
    tx.commit().await?;
    let today = today();
    rows.iter()
        .map(|row| map_task(row, today).map_err(OnboardingError::from))
        .collect()
}

pub async fn start_standard(
    pool: &TenantPool,
    tenant_id: TenantId,
    user_id: Uuid,
    employee_id: Uuid,
) -> Result<Vec<TaskView>, OnboardingError> {
    let mut tx = pool.begin_tenant(tenant_id).await?;
    let hire_date: Option<NaiveDate> =
        sqlx::query_scalar("SELECT hire_date FROM employees WHERE tenant_id = $1 AND id = $2")
            .bind(tenant_id.0)
            .bind(employee_id)
            .fetch_optional(&mut *tx)
            .await?;
    let Some(hire_date) = hire_date else {
        return Err(OnboardingError::EmployeeNotFound);
    };

    for (title, offset) in STANDARD_TASKS {
        sqlx::query(
            r#"INSERT INTO employee_onboarding_tasks
                 (tenant_id, employee_id, title, due_date, created_by)
               VALUES ($1, $2, $3, $4, $5)
               ON CONFLICT (tenant_id, employee_id, title) DO NOTHING"#,
        )
        .bind(tenant_id.0)
        .bind(employee_id)
        .bind(title)
        .bind(hire_date + Duration::days(*offset))
        .bind(user_id)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    list_tasks(pool, tenant_id, employee_id).await
}

pub async fn create_task(
    pool: &TenantPool,
    tenant_id: TenantId,
    user_id: Uuid,
    employee_id: Uuid,
    input: &TaskInput,
) -> Result<TaskView, OnboardingError> {
    let title = input.title.trim();
    if title.is_empty() || title.chars().count() > 200 {
        return Err(OnboardingError::InvalidTitle);
    }
    if input
        .notes
        .as_deref()
        .is_some_and(|n| n.chars().count() > 500)
    {
        return Err(OnboardingError::InvalidNotes);
    }

    let mut tx = pool.begin_tenant(tenant_id).await?;
    let employee: Option<Uuid> =
        sqlx::query_scalar("SELECT id FROM employees WHERE tenant_id = $1 AND id = $2")
            .bind(tenant_id.0)
            .bind(employee_id)
            .fetch_optional(&mut *tx)
            .await?;
    if employee.is_none() {
        return Err(OnboardingError::EmployeeNotFound);
    }

    let inserted = sqlx::query(
        r#"INSERT INTO employee_onboarding_tasks
             (tenant_id, employee_id, title, notes, due_date, created_by)
           VALUES ($1, $2, $3, $4, $5, $6)
           RETURNING id, employee_id, title, notes, due_date, status, completed_at"#,
    )
    .bind(tenant_id.0)
    .bind(employee_id)
    .bind(title)
    .bind(
        input
            .notes
            .as_deref()
            .map(str::trim)
            .filter(|v| !v.is_empty()),
    )
    .bind(input.due_date)
    .bind(user_id)
    .fetch_one(&mut *tx)
    .await;
    let row = match inserted {
        Ok(row) => row,
        Err(sqlx::Error::Database(e)) if e.is_unique_violation() => {
            return Err(OnboardingError::DuplicateTask)
        }
        Err(e) => return Err(e.into()),
    };
    let view = map_task(&row, today())?;
    tx.commit().await?;
    Ok(view)
}

pub async fn set_done(
    pool: &TenantPool,
    tenant_id: TenantId,
    user_id: Uuid,
    task_id: Uuid,
    done: bool,
) -> Result<TaskView, OnboardingError> {
    let mut tx = pool.begin_tenant(tenant_id).await?;
    let row = sqlx::query(
        r#"UPDATE employee_onboarding_tasks
              SET status = CASE WHEN $3 THEN 'done' ELSE 'pending' END,
                  completed_at = CASE WHEN $3 THEN now() ELSE NULL END,
                  completed_by = CASE WHEN $3 THEN $4 ELSE NULL END,
                  updated_at = now()
            WHERE tenant_id = $1 AND id = $2
            RETURNING id, employee_id, title, notes, due_date, status, completed_at"#,
    )
    .bind(tenant_id.0)
    .bind(task_id)
    .bind(done)
    .bind(user_id)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or(OnboardingError::NotFound)?;
    let view = map_task(&row, today())?;
    tx.commit().await?;
    Ok(view)
}

pub async fn delete_task(
    pool: &TenantPool,
    tenant_id: TenantId,
    task_id: Uuid,
) -> Result<(), OnboardingError> {
    let mut tx = pool.begin_tenant(tenant_id).await?;
    let deleted =
        sqlx::query("DELETE FROM employee_onboarding_tasks WHERE tenant_id = $1 AND id = $2")
            .bind(tenant_id.0)
            .bind(task_id)
            .execute(&mut *tx)
            .await?;
    if deleted.rows_affected() == 0 {
        return Err(OnboardingError::NotFound);
    }
    tx.commit().await?;
    Ok(())
}

pub async fn list_my_tasks(
    pool: &TenantPool,
    tenant_id: TenantId,
    user_id: Uuid,
) -> Result<Vec<TaskView>, OnboardingError> {
    let mut tx = pool.begin_tenant(tenant_id).await?;
    let rows = sqlx::query(
        r#"SELECT t.id, t.employee_id, t.title, t.notes, t.due_date, t.status, t.completed_at
             FROM employee_onboarding_tasks t
             JOIN employees e ON e.id = t.employee_id
            WHERE t.tenant_id = $1 AND e.user_id = $2
            ORDER BY (t.status = 'done'), t.due_date NULLS LAST, t.created_at"#,
    )
    .bind(tenant_id.0)
    .bind(user_id)
    .fetch_all(&mut *tx)
    .await?;
    tx.commit().await?;
    let today = today();
    rows.iter()
        .map(|row| map_task(row, today).map_err(OnboardingError::from))
        .collect()
}

pub async fn set_my_done(
    pool: &TenantPool,
    tenant_id: TenantId,
    user_id: Uuid,
    task_id: Uuid,
    done: bool,
) -> Result<TaskView, OnboardingError> {
    let mut tx = pool.begin_tenant(tenant_id).await?;
    let owned: Option<Uuid> = sqlx::query_scalar(
        r#"SELECT t.id FROM employee_onboarding_tasks t
             JOIN employees e ON e.id = t.employee_id
            WHERE t.tenant_id = $1 AND t.id = $2 AND e.user_id = $3"#,
    )
    .bind(tenant_id.0)
    .bind(task_id)
    .bind(user_id)
    .fetch_optional(&mut *tx)
    .await?;
    if owned.is_none() {
        return Err(OnboardingError::NotFound);
    }
    tx.commit().await?;
    set_done(pool, tenant_id, user_id, task_id, done).await
}
