//! Recruitment: job openings and candidates through a simple stage pipeline.
//! Tenant isolation is `begin_tenant` + explicit `tenant_id` predicates.

use chrono::{DateTime, Utc};
use serde::Serialize;
use sqlx::Row;
use talentos_db::TenantPool;
use talentos_kernel::TenantId;
use uuid::Uuid;

const EMPLOYMENT_TYPES: &[&str] = &["full_time", "part_time", "contract", "intern"];
pub const CANDIDATE_STAGES: &[&str] = &[
    "applied",
    "screening",
    "interview",
    "offer",
    "hired",
    "rejected",
];

#[derive(Debug, thiserror::Error)]
pub enum RecruitmentError {
    #[error("job opening not found")]
    JobNotFound,
    #[error("candidate not found")]
    CandidateNotFound,
    #[error("title must be 1-200 characters")]
    InvalidTitle,
    #[error("unsupported employment type")]
    InvalidEmploymentType,
    #[error("headcount must be between 1 and 100")]
    InvalidHeadcount,
    #[error("description must be at most 2000 characters")]
    InvalidDescription,
    #[error("first and last name must be 1-100 characters")]
    InvalidName,
    #[error("email must be at most 200 characters")]
    InvalidEmail,
    #[error("phone must be at most 30 characters")]
    InvalidPhone,
    #[error("source must be at most 100 characters")]
    InvalidSource,
    #[error("note must be at most 2000 characters")]
    InvalidNote,
    #[error("unsupported candidate stage")]
    InvalidStage,
    #[error("database error: {0}")]
    Db(#[from] sqlx::Error),
}

#[derive(Debug, Clone)]
pub struct JobInput {
    pub title: String,
    pub department: Option<String>,
    pub employment_type: String,
    pub headcount: i32,
    pub description: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct JobView {
    pub id: Uuid,
    pub title: String,
    pub department: Option<String>,
    pub employment_type: String,
    pub headcount: i32,
    pub status: String,
    pub description: Option<String>,
    pub candidate_count: i64,
    pub hired_count: i64,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct CandidateInput {
    pub first_name: String,
    pub last_name: String,
    pub email: Option<String>,
    pub phone: Option<String>,
    pub source: Option<String>,
    pub note: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct CandidateView {
    pub id: Uuid,
    pub job_opening_id: Uuid,
    pub first_name: String,
    pub last_name: String,
    pub email: Option<String>,
    pub phone: Option<String>,
    pub stage: String,
    pub source: Option<String>,
    pub note: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

fn trimmed(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|v| !v.is_empty())
}

fn map_job(row: &sqlx::postgres::PgRow) -> Result<JobView, sqlx::Error> {
    Ok(JobView {
        id: row.try_get("id")?,
        title: row.try_get("title")?,
        department: row.try_get("department")?,
        employment_type: row.try_get("employment_type")?,
        headcount: row.try_get("headcount")?,
        status: row.try_get("status")?,
        description: row.try_get("description")?,
        candidate_count: row.try_get("candidate_count")?,
        hired_count: row.try_get("hired_count")?,
        created_at: row.try_get("created_at")?,
    })
}

fn map_candidate(row: &sqlx::postgres::PgRow) -> Result<CandidateView, sqlx::Error> {
    Ok(CandidateView {
        id: row.try_get("id")?,
        job_opening_id: row.try_get("job_opening_id")?,
        first_name: row.try_get("first_name")?,
        last_name: row.try_get("last_name")?,
        email: row.try_get("email")?,
        phone: row.try_get("phone")?,
        stage: row.try_get("stage")?,
        source: row.try_get("source")?,
        note: row.try_get("note")?,
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
    })
}

pub async fn list_jobs(
    pool: &TenantPool,
    tenant_id: TenantId,
) -> Result<Vec<JobView>, RecruitmentError> {
    let mut tx = pool.begin_tenant(tenant_id).await?;
    let rows = sqlx::query(
        r#"SELECT j.id, j.title, j.department, j.employment_type, j.headcount, j.status,
                  j.description, j.created_at,
                  COUNT(c.id) AS candidate_count,
                  COUNT(c.id) FILTER (WHERE c.stage = 'hired') AS hired_count
             FROM job_openings j
             LEFT JOIN candidates c
                    ON c.job_opening_id = j.id AND c.tenant_id = j.tenant_id
            WHERE j.tenant_id = $1
            GROUP BY j.id
            ORDER BY (j.status = 'open') DESC, j.created_at DESC"#,
    )
    .bind(tenant_id.0)
    .fetch_all(&mut *tx)
    .await?;
    tx.commit().await?;
    rows.iter()
        .map(|row| map_job(row).map_err(RecruitmentError::from))
        .collect()
}

pub async fn create_job(
    pool: &TenantPool,
    tenant_id: TenantId,
    user_id: Uuid,
    input: &JobInput,
) -> Result<JobView, RecruitmentError> {
    let title = input.title.trim();
    if title.is_empty() || title.chars().count() > 200 {
        return Err(RecruitmentError::InvalidTitle);
    }
    if !EMPLOYMENT_TYPES.contains(&input.employment_type.as_str()) {
        return Err(RecruitmentError::InvalidEmploymentType);
    }
    if !(1..=100).contains(&input.headcount) {
        return Err(RecruitmentError::InvalidHeadcount);
    }
    if input
        .description
        .as_deref()
        .is_some_and(|d| d.chars().count() > 2000)
    {
        return Err(RecruitmentError::InvalidDescription);
    }

    let mut tx = pool.begin_tenant(tenant_id).await?;
    let row = sqlx::query(
        r#"INSERT INTO job_openings
             (tenant_id, title, department, employment_type, headcount, description, created_by)
           VALUES ($1,$2,$3,$4,$5,$6,$7)
           RETURNING id, title, department, employment_type, headcount, status, description,
                     created_at, 0::bigint AS candidate_count, 0::bigint AS hired_count"#,
    )
    .bind(tenant_id.0)
    .bind(title)
    .bind(trimmed(input.department.as_deref()))
    .bind(&input.employment_type)
    .bind(input.headcount)
    .bind(trimmed(input.description.as_deref()))
    .bind(user_id)
    .fetch_one(&mut *tx)
    .await?;
    let view = map_job(&row)?;
    tx.commit().await?;
    Ok(view)
}

pub async fn set_job_status(
    pool: &TenantPool,
    tenant_id: TenantId,
    job_id: Uuid,
    open: bool,
) -> Result<(), RecruitmentError> {
    let mut tx = pool.begin_tenant(tenant_id).await?;
    let updated = sqlx::query(
        "UPDATE job_openings SET status = $3, updated_at = now()
          WHERE tenant_id = $1 AND id = $2",
    )
    .bind(tenant_id.0)
    .bind(job_id)
    .bind(if open { "open" } else { "closed" })
    .execute(&mut *tx)
    .await?;
    if updated.rows_affected() == 0 {
        return Err(RecruitmentError::JobNotFound);
    }
    tx.commit().await?;
    Ok(())
}

pub async fn delete_job(
    pool: &TenantPool,
    tenant_id: TenantId,
    job_id: Uuid,
) -> Result<(), RecruitmentError> {
    let mut tx = pool.begin_tenant(tenant_id).await?;
    let deleted = sqlx::query("DELETE FROM job_openings WHERE tenant_id = $1 AND id = $2")
        .bind(tenant_id.0)
        .bind(job_id)
        .execute(&mut *tx)
        .await?;
    if deleted.rows_affected() == 0 {
        return Err(RecruitmentError::JobNotFound);
    }
    tx.commit().await?;
    Ok(())
}

pub async fn list_candidates(
    pool: &TenantPool,
    tenant_id: TenantId,
    job_id: Uuid,
) -> Result<Vec<CandidateView>, RecruitmentError> {
    let mut tx = pool.begin_tenant(tenant_id).await?;
    let exists: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM job_openings WHERE tenant_id = $1 AND id = $2)",
    )
    .bind(tenant_id.0)
    .bind(job_id)
    .fetch_one(&mut *tx)
    .await?;
    if !exists {
        return Err(RecruitmentError::JobNotFound);
    }
    let rows = sqlx::query(
        r#"SELECT id, job_opening_id, first_name, last_name, email, phone, stage,
                  source, note, created_at, updated_at
             FROM candidates
            WHERE tenant_id = $1 AND job_opening_id = $2
            ORDER BY CASE stage
                       WHEN 'applied' THEN 0 WHEN 'screening' THEN 1
                       WHEN 'interview' THEN 2 WHEN 'offer' THEN 3
                       WHEN 'hired' THEN 4 ELSE 5 END,
                     created_at"#,
    )
    .bind(tenant_id.0)
    .bind(job_id)
    .fetch_all(&mut *tx)
    .await?;
    tx.commit().await?;
    rows.iter()
        .map(|row| map_candidate(row).map_err(RecruitmentError::from))
        .collect()
}

pub async fn create_candidate(
    pool: &TenantPool,
    tenant_id: TenantId,
    user_id: Uuid,
    job_id: Uuid,
    input: &CandidateInput,
) -> Result<CandidateView, RecruitmentError> {
    let first = input.first_name.trim();
    let last = input.last_name.trim();
    if first.is_empty()
        || last.is_empty()
        || first.chars().count() > 100
        || last.chars().count() > 100
    {
        return Err(RecruitmentError::InvalidName);
    }
    if input
        .email
        .as_deref()
        .is_some_and(|v| v.chars().count() > 200)
    {
        return Err(RecruitmentError::InvalidEmail);
    }
    if input
        .phone
        .as_deref()
        .is_some_and(|v| v.chars().count() > 30)
    {
        return Err(RecruitmentError::InvalidPhone);
    }
    if input
        .source
        .as_deref()
        .is_some_and(|v| v.chars().count() > 100)
    {
        return Err(RecruitmentError::InvalidSource);
    }
    if input
        .note
        .as_deref()
        .is_some_and(|v| v.chars().count() > 2000)
    {
        return Err(RecruitmentError::InvalidNote);
    }

    let mut tx = pool.begin_tenant(tenant_id).await?;
    let exists: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM job_openings WHERE tenant_id = $1 AND id = $2)",
    )
    .bind(tenant_id.0)
    .bind(job_id)
    .fetch_one(&mut *tx)
    .await?;
    if !exists {
        return Err(RecruitmentError::JobNotFound);
    }
    let row = sqlx::query(
        r#"INSERT INTO candidates
             (tenant_id, job_opening_id, first_name, last_name, email, phone, source, note, created_by)
           VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9)
           RETURNING id, job_opening_id, first_name, last_name, email, phone, stage,
                     source, note, created_at, updated_at"#,
    )
    .bind(tenant_id.0)
    .bind(job_id)
    .bind(first)
    .bind(last)
    .bind(trimmed(input.email.as_deref()))
    .bind(trimmed(input.phone.as_deref()))
    .bind(trimmed(input.source.as_deref()))
    .bind(trimmed(input.note.as_deref()))
    .bind(user_id)
    .fetch_one(&mut *tx)
    .await?;
    let view = map_candidate(&row)?;
    tx.commit().await?;
    Ok(view)
}

pub async fn set_candidate_stage(
    pool: &TenantPool,
    tenant_id: TenantId,
    candidate_id: Uuid,
    stage: &str,
) -> Result<CandidateView, RecruitmentError> {
    if !CANDIDATE_STAGES.contains(&stage) {
        return Err(RecruitmentError::InvalidStage);
    }
    let mut tx = pool.begin_tenant(tenant_id).await?;
    let row = sqlx::query(
        r#"UPDATE candidates SET stage = $3, updated_at = now()
            WHERE tenant_id = $1 AND id = $2
            RETURNING id, job_opening_id, first_name, last_name, email, phone, stage,
                      source, note, created_at, updated_at"#,
    )
    .bind(tenant_id.0)
    .bind(candidate_id)
    .bind(stage)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or(RecruitmentError::CandidateNotFound)?;
    let view = map_candidate(&row)?;
    tx.commit().await?;
    Ok(view)
}

pub async fn delete_candidate(
    pool: &TenantPool,
    tenant_id: TenantId,
    candidate_id: Uuid,
) -> Result<(), RecruitmentError> {
    let mut tx = pool.begin_tenant(tenant_id).await?;
    let deleted = sqlx::query("DELETE FROM candidates WHERE tenant_id = $1 AND id = $2")
        .bind(tenant_id.0)
        .bind(candidate_id)
        .execute(&mut *tx)
        .await?;
    if deleted.rows_affected() == 0 {
        return Err(RecruitmentError::CandidateNotFound);
    }
    tx.commit().await?;
    Ok(())
}
