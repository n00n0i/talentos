//! Recruitment: job openings and candidates through a simple stage pipeline.
//! Tenant isolation is `begin_tenant` + explicit `tenant_id` predicates.

use std::path::PathBuf;

use chrono::{DateTime, Utc};
use serde::Serialize;
use sqlx::Row;
use talentos_db::TenantPool;
use talentos_kernel::TenantId;
use uuid::Uuid;

const INTERVIEW_MODES: &[&str] = &["onsite", "online", "phone"];
const INTERVIEW_STATUSES: &[&str] = &["scheduled", "completed", "cancelled"];

const MAX_CV_BYTES: usize = 5 * 1024 * 1024;
const ALLOWED_CV_TYPES: &[&str] = &["application/pdf", "image/jpeg", "image/png"];

pub fn cv_dir() -> PathBuf {
    std::env::var("CANDIDATE_CV_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/tmp/talentos-cvs"))
}

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
    #[error("department must be at most 100 characters")]
    InvalidDepartment,
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
    #[error("unsupported interview mode")]
    InvalidMode,
    #[error("unsupported interview status")]
    InvalidInterviewStatus,
    #[error("interview not found")]
    InterviewNotFound,
    #[error("location must be at most 500 characters")]
    InvalidLocation,
    #[error("notes must be at most 1000 characters")]
    InvalidNotes,
    #[error("CV is too large; maximum size is 5 MB")]
    CvTooLarge,
    #[error("unsupported CV type; use PDF, JPEG, or PNG")]
    InvalidCvType,
    #[error("candidate has no CV")]
    NoCv,
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
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

#[derive(Debug, Clone)]
pub struct InterviewInput {
    pub scheduled_at: DateTime<Utc>,
    pub mode: String,
    pub location: Option<String>,
    pub interviewer_user_id: Option<Uuid>,
    pub notes: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct InterviewView {
    pub id: Uuid,
    pub candidate_id: Uuid,
    pub scheduled_at: DateTime<Utc>,
    pub mode: String,
    pub location: Option<String>,
    pub interviewer_user_id: Option<Uuid>,
    pub status: String,
    pub notes: Option<String>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Serialize)]
pub struct StageEventView {
    pub id: i64,
    pub from_stage: Option<String>,
    pub to_stage: String,
    pub changed_by: Option<Uuid>,
    pub changed_at: DateTime<Utc>,
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
    pub has_cv: bool,
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
        has_cv: row.try_get("has_cv")?,
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
    let department = trimmed(input.department.as_deref());
    if department.is_some_and(|d| d.chars().count() > 100) {
        return Err(RecruitmentError::InvalidDepartment);
    }
    let description = trimmed(input.description.as_deref());
    if description.is_some_and(|d| d.chars().count() > 2000) {
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
    .bind(department)
    .bind(&input.employment_type)
    .bind(input.headcount)
    .bind(description)
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
    let cv_paths: Vec<String> = sqlx::query_scalar(
        "SELECT cv_path FROM candidates
          WHERE tenant_id = $1 AND job_opening_id = $2 AND cv_path IS NOT NULL",
    )
    .bind(tenant_id.0)
    .bind(job_id)
    .fetch_all(&mut *tx)
    .await?;
    let deleted = sqlx::query("DELETE FROM job_openings WHERE tenant_id = $1 AND id = $2")
        .bind(tenant_id.0)
        .bind(job_id)
        .execute(&mut *tx)
        .await?;
    if deleted.rows_affected() == 0 {
        return Err(RecruitmentError::JobNotFound);
    }
    tx.commit().await?;
    for path in cv_paths {
        let _ = tokio::fs::remove_file(path).await;
    }
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
                  source, note, (cv_path IS NOT NULL) AS has_cv, created_at, updated_at
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
    let email = trimmed(input.email.as_deref());
    let phone = trimmed(input.phone.as_deref());
    let source = trimmed(input.source.as_deref());
    let note = trimmed(input.note.as_deref());
    if email.is_some_and(|v| v.chars().count() > 200) {
        return Err(RecruitmentError::InvalidEmail);
    }
    if phone.is_some_and(|v| v.chars().count() > 30) {
        return Err(RecruitmentError::InvalidPhone);
    }
    if source.is_some_and(|v| v.chars().count() > 100) {
        return Err(RecruitmentError::InvalidSource);
    }
    if note.is_some_and(|v| v.chars().count() > 2000) {
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
                     source, note, (cv_path IS NOT NULL) AS has_cv, created_at, updated_at"#,
    )
    .bind(tenant_id.0)
    .bind(job_id)
    .bind(first)
    .bind(last)
    .bind(email)
    .bind(phone)
    .bind(source)
    .bind(note)
    .bind(user_id)
    .fetch_one(&mut *tx)
    .await?;
    let view = map_candidate(&row)?;
    sqlx::query(
        "INSERT INTO candidate_stage_events
           (tenant_id, candidate_id, from_stage, to_stage, changed_by)
         VALUES ($1, $2, NULL, 'applied', $3)",
    )
    .bind(tenant_id.0)
    .bind(view.id)
    .bind(user_id)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(view)
}

pub async fn set_candidate_stage(
    pool: &TenantPool,
    tenant_id: TenantId,
    user_id: Uuid,
    candidate_id: Uuid,
    stage: &str,
) -> Result<CandidateView, RecruitmentError> {
    if !CANDIDATE_STAGES.contains(&stage) {
        return Err(RecruitmentError::InvalidStage);
    }
    let mut tx = pool.begin_tenant(tenant_id).await?;
    let old: Option<String> = sqlx::query_scalar(
        "SELECT stage FROM candidates WHERE tenant_id = $1 AND id = $2 FOR UPDATE",
    )
    .bind(tenant_id.0)
    .bind(candidate_id)
    .fetch_optional(&mut *tx)
    .await?;
    let Some(old) = old else {
        return Err(RecruitmentError::CandidateNotFound);
    };
    let row = sqlx::query(
        r#"UPDATE candidates SET stage = $3, updated_at = now()
            WHERE tenant_id = $1 AND id = $2
            RETURNING id, job_opening_id, first_name, last_name, email, phone, stage,
                      source, note, (cv_path IS NOT NULL) AS has_cv, created_at, updated_at"#,
    )
    .bind(tenant_id.0)
    .bind(candidate_id)
    .bind(stage)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or(RecruitmentError::CandidateNotFound)?;
    let view = map_candidate(&row)?;
    if old != stage {
        sqlx::query(
            "INSERT INTO candidate_stage_events
               (tenant_id, candidate_id, from_stage, to_stage, changed_by)
             VALUES ($1, $2, $3, $4, $5)",
        )
        .bind(tenant_id.0)
        .bind(candidate_id)
        .bind(&old)
        .bind(stage)
        .bind(user_id)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    Ok(view)
}

pub async fn list_stage_history(
    pool: &TenantPool,
    tenant_id: TenantId,
    candidate_id: Uuid,
) -> Result<Vec<StageEventView>, RecruitmentError> {
    let mut tx = pool.begin_tenant(tenant_id).await?;
    let exists: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM candidates WHERE tenant_id = $1 AND id = $2)",
    )
    .bind(tenant_id.0)
    .bind(candidate_id)
    .fetch_one(&mut *tx)
    .await?;
    if !exists {
        return Err(RecruitmentError::CandidateNotFound);
    }
    let rows = sqlx::query(
        r#"SELECT id, from_stage, to_stage, changed_by, changed_at
             FROM candidate_stage_events
            WHERE tenant_id = $1 AND candidate_id = $2
            ORDER BY changed_at DESC, id DESC"#,
    )
    .bind(tenant_id.0)
    .bind(candidate_id)
    .fetch_all(&mut *tx)
    .await?;
    tx.commit().await?;
    rows.iter()
        .map(|row| {
            Ok(StageEventView {
                id: row.try_get("id")?,
                from_stage: row.try_get("from_stage")?,
                to_stage: row.try_get("to_stage")?,
                changed_by: row.try_get("changed_by")?,
                changed_at: row.try_get("changed_at")?,
            })
        })
        .collect::<Result<Vec<_>, sqlx::Error>>()
        .map_err(RecruitmentError::from)
}

pub async fn delete_candidate(
    pool: &TenantPool,
    tenant_id: TenantId,
    candidate_id: Uuid,
) -> Result<(), RecruitmentError> {
    let mut tx = pool.begin_tenant(tenant_id).await?;
    let cv_path: Option<String> = sqlx::query_scalar(
        "DELETE FROM candidates WHERE tenant_id = $1 AND id = $2 RETURNING cv_path",
    )
    .bind(tenant_id.0)
    .bind(candidate_id)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or(RecruitmentError::CandidateNotFound)?;
    tx.commit().await?;
    if let Some(path) = cv_path {
        let _ = tokio::fs::remove_file(path).await;
    }
    Ok(())
}

pub struct CandidateFile {
    pub content_type: String,
    pub bytes: Vec<u8>,
}

async fn write_private(
    dir: &std::path::Path,
    path: &std::path::Path,
    bytes: &[u8],
) -> std::io::Result<()> {
    tokio::fs::create_dir_all(dir).await?;
    tokio::fs::write(path, bytes).await?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        tokio::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700)).await?;
        tokio::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)).await?;
    }
    Ok(())
}

pub async fn set_cv(
    pool: &TenantPool,
    tenant_id: TenantId,
    candidate_id: Uuid,
    content_type: &str,
    bytes: &[u8],
) -> Result<(), RecruitmentError> {
    if bytes.len() > MAX_CV_BYTES {
        return Err(RecruitmentError::CvTooLarge);
    }
    if !ALLOWED_CV_TYPES.contains(&content_type) {
        return Err(RecruitmentError::InvalidCvType);
    }
    let ext = match content_type {
        "image/jpeg" => "jpg",
        "image/png" => "png",
        _ => "pdf",
    };

    let mut tx = pool.begin_tenant(tenant_id).await?;
    let existing: Option<String> = sqlx::query_scalar(
        "SELECT cv_path FROM candidates WHERE tenant_id = $1 AND id = $2 FOR UPDATE",
    )
    .bind(tenant_id.0)
    .bind(candidate_id)
    .fetch_optional(&mut *tx)
    .await?
    .flatten();
    let exists: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM candidates WHERE tenant_id = $1 AND id = $2)",
    )
    .bind(tenant_id.0)
    .bind(candidate_id)
    .fetch_one(&mut *tx)
    .await?;
    if !exists {
        return Err(RecruitmentError::CandidateNotFound);
    }

    let dir = cv_dir();
    let filename = format!("{}.{}", Uuid::new_v4(), ext);
    let path = dir.join(&filename);
    write_private(&dir, &path, bytes).await?;

    let updated = sqlx::query(
        "UPDATE candidates SET cv_path = $3, cv_type = $4, updated_at = now()
          WHERE tenant_id = $1 AND id = $2 RETURNING id",
    )
    .bind(tenant_id.0)
    .bind(candidate_id)
    .bind(path.to_string_lossy().to_string())
    .bind(content_type)
    .fetch_optional(&mut *tx)
    .await?;
    if updated.is_none() {
        let _ = tokio::fs::remove_file(&path).await;
        return Err(RecruitmentError::CandidateNotFound);
    }
    tx.commit().await?;
    if let Some(old) = existing {
        let _ = tokio::fs::remove_file(old).await;
    }
    Ok(())
}

pub async fn get_cv(
    pool: &TenantPool,
    tenant_id: TenantId,
    candidate_id: Uuid,
) -> Result<CandidateFile, RecruitmentError> {
    let mut tx = pool.begin_tenant(tenant_id).await?;
    let row =
        sqlx::query("SELECT cv_path, cv_type FROM candidates WHERE tenant_id = $1 AND id = $2")
            .bind(tenant_id.0)
            .bind(candidate_id)
            .fetch_optional(&mut *tx)
            .await?;
    tx.commit().await?;
    let Some(row) = row else {
        return Err(RecruitmentError::CandidateNotFound);
    };
    let path: Option<String> = row.try_get("cv_path")?;
    let Some(path) = path else {
        return Err(RecruitmentError::NoCv);
    };
    let content_type: Option<String> = row.try_get("cv_type")?;
    let bytes = match tokio::fs::read(path).await {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Err(RecruitmentError::CandidateNotFound)
        }
        Err(e) => return Err(e.into()),
    };
    Ok(CandidateFile {
        content_type: content_type.unwrap_or_else(|| "application/octet-stream".into()),
        bytes,
    })
}

fn map_interview(row: &sqlx::postgres::PgRow) -> Result<InterviewView, sqlx::Error> {
    Ok(InterviewView {
        id: row.try_get("id")?,
        candidate_id: row.try_get("candidate_id")?,
        scheduled_at: row.try_get("scheduled_at")?,
        mode: row.try_get("mode")?,
        location: row.try_get("location")?,
        interviewer_user_id: row.try_get("interviewer_user_id")?,
        status: row.try_get("status")?,
        notes: row.try_get("notes")?,
        created_at: row.try_get("created_at")?,
    })
}

const INTERVIEW_SELECT: &str = r#"SELECT id, candidate_id, scheduled_at, mode, location,
              interviewer_user_id, status, notes, created_at
         FROM candidate_interviews
        WHERE tenant_id = $1"#;

pub async fn schedule_interview(
    pool: &TenantPool,
    tenant_id: TenantId,
    user_id: Uuid,
    candidate_id: Uuid,
    input: &InterviewInput,
) -> Result<InterviewView, RecruitmentError> {
    if !INTERVIEW_MODES.contains(&input.mode.as_str()) {
        return Err(RecruitmentError::InvalidMode);
    }
    let location = input
        .location
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty());
    if location.is_some_and(|v| v.chars().count() > 500) {
        return Err(RecruitmentError::InvalidLocation);
    }
    let notes = input
        .notes
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty());
    if notes.is_some_and(|v| v.chars().count() > 1000) {
        return Err(RecruitmentError::InvalidNotes);
    }

    let mut tx = pool.begin_tenant(tenant_id).await?;
    let exists: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM candidates WHERE tenant_id = $1 AND id = $2)",
    )
    .bind(tenant_id.0)
    .bind(candidate_id)
    .fetch_one(&mut *tx)
    .await?;
    if !exists {
        return Err(RecruitmentError::CandidateNotFound);
    }
    let row = sqlx::query(
        r#"INSERT INTO candidate_interviews
             (tenant_id, candidate_id, scheduled_at, mode, location, interviewer_user_id,
              notes, created_by)
           VALUES ($1,$2,$3,$4,$5,$6,$7,$8)
           RETURNING id, candidate_id, scheduled_at, mode, location, interviewer_user_id,
                     status, notes, created_at"#,
    )
    .bind(tenant_id.0)
    .bind(candidate_id)
    .bind(input.scheduled_at)
    .bind(&input.mode)
    .bind(location)
    .bind(input.interviewer_user_id)
    .bind(notes)
    .bind(user_id)
    .fetch_one(&mut *tx)
    .await?;
    let view = map_interview(&row)?;
    tx.commit().await?;
    Ok(view)
}

pub async fn list_interviews(
    pool: &TenantPool,
    tenant_id: TenantId,
    candidate_id: Uuid,
) -> Result<Vec<InterviewView>, RecruitmentError> {
    let mut tx = pool.begin_tenant(tenant_id).await?;
    let exists: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM candidates WHERE tenant_id = $1 AND id = $2)",
    )
    .bind(tenant_id.0)
    .bind(candidate_id)
    .fetch_one(&mut *tx)
    .await?;
    if !exists {
        return Err(RecruitmentError::CandidateNotFound);
    }
    let sql = format!("{INTERVIEW_SELECT} AND candidate_id = $2 ORDER BY scheduled_at DESC");
    let rows = sqlx::query(&sql)
        .bind(tenant_id.0)
        .bind(candidate_id)
        .fetch_all(&mut *tx)
        .await?;
    tx.commit().await?;
    rows.iter()
        .map(|row| map_interview(row).map_err(RecruitmentError::from))
        .collect()
}

pub async fn set_interview_status(
    pool: &TenantPool,
    tenant_id: TenantId,
    interview_id: Uuid,
    status: &str,
) -> Result<InterviewView, RecruitmentError> {
    if !INTERVIEW_STATUSES.contains(&status) {
        return Err(RecruitmentError::InvalidInterviewStatus);
    }
    let mut tx = pool.begin_tenant(tenant_id).await?;
    let row = sqlx::query(
        r#"UPDATE candidate_interviews SET status = $3, updated_at = now()
            WHERE tenant_id = $1 AND id = $2
            RETURNING id, candidate_id, scheduled_at, mode, location, interviewer_user_id,
                      status, notes, created_at"#,
    )
    .bind(tenant_id.0)
    .bind(interview_id)
    .bind(status)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or(RecruitmentError::InterviewNotFound)?;
    let view = map_interview(&row)?;
    tx.commit().await?;
    Ok(view)
}

pub async fn delete_interview(
    pool: &TenantPool,
    tenant_id: TenantId,
    interview_id: Uuid,
) -> Result<(), RecruitmentError> {
    let mut tx = pool.begin_tenant(tenant_id).await?;
    let deleted = sqlx::query("DELETE FROM candidate_interviews WHERE tenant_id = $1 AND id = $2")
        .bind(tenant_id.0)
        .bind(interview_id)
        .execute(&mut *tx)
        .await?;
    if deleted.rows_affected() == 0 {
        return Err(RecruitmentError::InterviewNotFound);
    }
    tx.commit().await?;
    Ok(())
}
