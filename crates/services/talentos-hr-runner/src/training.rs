//! Employee training history and certificate-expiry views. Tenant isolation is
//! `begin_tenant` + explicit `tenant_id` predicates.

use chrono::{DateTime, Duration, NaiveDate, Utc};
use serde::Serialize;
use sqlx::Row;
use talentos_attendance::bangkok_offset;
use talentos_db::TenantPool;
use talentos_kernel::TenantId;
use uuid::Uuid;

#[derive(Debug, thiserror::Error)]
pub enum TrainingError {
    #[error("employee not found for this tenant")]
    EmployeeNotFound,
    #[error("training record not found")]
    NotFound,
    #[error("title must be 1-200 characters")]
    InvalidTitle,
    #[error("provider must be at most 200 characters")]
    InvalidProvider,
    #[error("expiry date must not precede the completion date")]
    InvalidDates,
    #[error("database error: {0}")]
    Db(#[from] sqlx::Error),
}

#[derive(Debug, Clone)]
pub struct TrainingInput {
    pub title: String,
    pub provider: Option<String>,
    pub completed_on: Option<NaiveDate>,
    pub expires_on: Option<NaiveDate>,
}

#[derive(Debug, Serialize)]
pub struct TrainingView {
    pub id: Uuid,
    pub employee_id: Uuid,
    pub title: String,
    pub provider: Option<String>,
    pub completed_on: Option<NaiveDate>,
    pub expires_on: Option<NaiveDate>,
    /// Days until expiry; negative when expired. `None` when no expiry.
    pub days_left: Option<i64>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Serialize)]
pub struct ExpiringTraining {
    pub employee_code: String,
    pub employee_name: String,
    #[serde(flatten)]
    pub training: TrainingView,
}

fn today() -> NaiveDate {
    Utc::now().with_timezone(&bangkok_offset()).date_naive()
}

fn map_training(
    row: &sqlx::postgres::PgRow,
    today: NaiveDate,
) -> Result<TrainingView, sqlx::Error> {
    let expires_on: Option<NaiveDate> = row.try_get("expires_on")?;
    Ok(TrainingView {
        id: row.try_get("id")?,
        employee_id: row.try_get("employee_id")?,
        title: row.try_get("title")?,
        provider: row.try_get("provider")?,
        completed_on: row.try_get("completed_on")?,
        expires_on,
        days_left: expires_on.map(|d| (d - today).num_days()),
        created_at: row.try_get("created_at")?,
    })
}

pub async fn list_training(
    pool: &TenantPool,
    tenant_id: TenantId,
    employee_id: Uuid,
) -> Result<Vec<TrainingView>, TrainingError> {
    let mut tx = pool.begin_tenant(tenant_id).await?;
    let rows = sqlx::query(
        r#"SELECT id, employee_id, title, provider, completed_on, expires_on, created_at
             FROM employee_training_records
            WHERE tenant_id = $1 AND employee_id = $2
            ORDER BY completed_on DESC NULLS LAST, created_at DESC"#,
    )
    .bind(tenant_id.0)
    .bind(employee_id)
    .fetch_all(&mut *tx)
    .await?;
    tx.commit().await?;
    let today = today();
    rows.iter()
        .map(|row| map_training(row, today).map_err(TrainingError::from))
        .collect()
}

pub async fn create_training(
    pool: &TenantPool,
    tenant_id: TenantId,
    user_id: Uuid,
    employee_id: Uuid,
    input: &TrainingInput,
) -> Result<TrainingView, TrainingError> {
    let title = input.title.trim();
    if title.is_empty() || title.chars().count() > 200 {
        return Err(TrainingError::InvalidTitle);
    }
    let provider = input
        .provider
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty());
    if provider.is_some_and(|p| p.chars().count() > 200) {
        return Err(TrainingError::InvalidProvider);
    }
    if let (Some(completed), Some(expires)) = (input.completed_on, input.expires_on) {
        if expires < completed {
            return Err(TrainingError::InvalidDates);
        }
    }

    let mut tx = pool.begin_tenant(tenant_id).await?;
    let exists: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM employees WHERE tenant_id = $1 AND id = $2)",
    )
    .bind(tenant_id.0)
    .bind(employee_id)
    .fetch_one(&mut *tx)
    .await?;
    if !exists {
        return Err(TrainingError::EmployeeNotFound);
    }
    let row = sqlx::query(
        r#"INSERT INTO employee_training_records
             (tenant_id, employee_id, title, provider, completed_on, expires_on, created_by)
           VALUES ($1,$2,$3,$4,$5,$6,$7)
           RETURNING id, employee_id, title, provider, completed_on, expires_on, created_at"#,
    )
    .bind(tenant_id.0)
    .bind(employee_id)
    .bind(title)
    .bind(provider)
    .bind(input.completed_on)
    .bind(input.expires_on)
    .bind(user_id)
    .fetch_one(&mut *tx)
    .await?;
    let view = map_training(&row, today())?;
    tx.commit().await?;
    Ok(view)
}

pub async fn delete_training(
    pool: &TenantPool,
    tenant_id: TenantId,
    training_id: Uuid,
) -> Result<(), TrainingError> {
    let mut tx = pool.begin_tenant(tenant_id).await?;
    let deleted =
        sqlx::query("DELETE FROM employee_training_records WHERE tenant_id = $1 AND id = $2")
            .bind(tenant_id.0)
            .bind(training_id)
            .execute(&mut *tx)
            .await?;
    if deleted.rows_affected() == 0 {
        return Err(TrainingError::NotFound);
    }
    tx.commit().await?;
    Ok(())
}

pub async fn expiring_training(
    pool: &TenantPool,
    tenant_id: TenantId,
    days: i64,
) -> Result<Vec<ExpiringTraining>, TrainingError> {
    let days = days.clamp(1, 365);
    let today = today();
    let limit = today + Duration::days(days);
    let floor = today - Duration::days(365);
    let mut tx = pool.begin_tenant(tenant_id).await?;
    let rows = sqlx::query(
        r#"SELECT t.id, t.employee_id, t.title, t.provider, t.completed_on, t.expires_on,
                  t.created_at, e.employee_code, e.first_name_th, e.last_name_th
             FROM employee_training_records t
             JOIN employees e ON e.id = t.employee_id AND e.tenant_id = t.tenant_id
            WHERE t.tenant_id = $1 AND t.expires_on IS NOT NULL
              AND t.expires_on BETWEEN $2 AND $3
              AND e.status <> 'terminated'
            ORDER BY t.expires_on, e.employee_code
            LIMIT 500"#,
    )
    .bind(tenant_id.0)
    .bind(floor)
    .bind(limit)
    .fetch_all(&mut *tx)
    .await?;
    tx.commit().await?;

    rows.iter()
        .map(|row| {
            Ok(ExpiringTraining {
                employee_code: row.try_get("employee_code")?,
                employee_name: format!(
                    "{} {}",
                    row.try_get::<String, _>("first_name_th")?,
                    row.try_get::<String, _>("last_name_th")?
                ),
                training: map_training(row, today)?,
            })
        })
        .collect::<Result<Vec<_>, sqlx::Error>>()
        .map_err(TrainingError::from)
}

pub async fn list_my_training(
    pool: &TenantPool,
    tenant_id: TenantId,
    user_id: Uuid,
) -> Result<Vec<TrainingView>, TrainingError> {
    let mut tx = pool.begin_tenant(tenant_id).await?;
    let rows = sqlx::query(
        r#"SELECT t.id, t.employee_id, t.title, t.provider, t.completed_on, t.expires_on,
                  t.created_at
             FROM employee_training_records t
             JOIN employees e ON e.id = t.employee_id AND e.tenant_id = t.tenant_id
            WHERE t.tenant_id = $1 AND e.user_id = $2
            ORDER BY t.completed_on DESC NULLS LAST, t.created_at DESC"#,
    )
    .bind(tenant_id.0)
    .bind(user_id)
    .fetch_all(&mut *tx)
    .await?;
    tx.commit().await?;
    let today = today();
    rows.iter()
        .map(|row| map_training(row, today).map_err(TrainingError::from))
        .collect()
}
