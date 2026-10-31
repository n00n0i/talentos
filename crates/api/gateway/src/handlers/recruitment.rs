//! Recruitment: job openings and candidate pipeline.

use axum::{
    body::Bytes,
    extract::{Path, State},
    http::{header, HeaderMap, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

use crate::dto::responses::ErrorResponse;
use crate::middleware::auth::{require, Authed};
use crate::state::AppState;
use talentos_hr_runner::recruitment::{
    self, CandidateFile, CandidateInput, InterviewInput, JobInput, RecruitmentError,
};

#[derive(Debug, Deserialize)]
pub struct CreateJobBody {
    pub title: String,
    pub department: Option<String>,
    pub employment_type: Option<String>,
    pub headcount: Option<i32>,
    pub description: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct JobStatusBody {
    pub status: String,
}

#[derive(Debug, Deserialize)]
pub struct CreateCandidateBody {
    pub first_name: String,
    pub last_name: String,
    pub email: Option<String>,
    pub phone: Option<String>,
    pub source: Option<String>,
    pub note: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ScheduleInterviewBody {
    pub scheduled_at: DateTime<Utc>,
    pub mode: String,
    pub location: Option<String>,
    pub interviewer_user_id: Option<Uuid>,
    pub notes: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct InterviewStatusBody {
    pub status: String,
}

#[derive(Debug, Deserialize)]
pub struct StageBody {
    pub stage: String,
}

/// GET /api/v1/jobs
pub async fn list_jobs(State(state): State<AppState>, Authed(ctx): Authed) -> Response {
    if let Err(forbidden) = require(&ctx, "employee", "read_all") {
        return forbidden;
    }
    match recruitment::list_jobs(&state.pool, ctx.tenant_id).await {
        Ok(jobs) => Json(json!({ "jobs": jobs })).into_response(),
        Err(e) => map_error(e),
    }
}

/// POST /api/v1/jobs
pub async fn create_job(
    State(state): State<AppState>,
    Authed(ctx): Authed,
    Json(body): Json<CreateJobBody>,
) -> Response {
    if let Err(forbidden) = require(&ctx, "employee", "update") {
        return forbidden;
    }
    let input = JobInput {
        title: body.title,
        department: body.department,
        employment_type: body.employment_type.unwrap_or_else(|| "full_time".into()),
        headcount: body.headcount.unwrap_or(1),
        description: body.description,
    };
    match recruitment::create_job(&state.pool, ctx.tenant_id, ctx.user_id.0, &input).await {
        Ok(job) => (StatusCode::CREATED, Json(json!({ "job": job }))).into_response(),
        Err(e) => map_error(e),
    }
}

/// PATCH /api/v1/jobs/:id
pub async fn update_job(
    State(state): State<AppState>,
    Authed(ctx): Authed,
    Path(id): Path<Uuid>,
    Json(body): Json<JobStatusBody>,
) -> Response {
    if let Err(forbidden) = require(&ctx, "employee", "update") {
        return forbidden;
    }
    let open = match body.status.as_str() {
        "open" => true,
        "closed" => false,
        _ => return invalid("status must be 'open' or 'closed'"),
    };
    match recruitment::set_job_status(&state.pool, ctx.tenant_id, id, open).await {
        Ok(()) => Json(json!({ "id": id.to_string(), "status": body.status })).into_response(),
        Err(e) => map_error(e),
    }
}

/// DELETE /api/v1/jobs/:id
pub async fn remove_job(
    State(state): State<AppState>,
    Authed(ctx): Authed,
    Path(id): Path<Uuid>,
) -> Response {
    if let Err(forbidden) = require(&ctx, "employee", "update") {
        return forbidden;
    }
    match recruitment::delete_job(&state.pool, ctx.tenant_id, id).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => map_error(e),
    }
}

/// GET /api/v1/jobs/:id/candidates
pub async fn list_candidates(
    State(state): State<AppState>,
    Authed(ctx): Authed,
    Path(id): Path<Uuid>,
) -> Response {
    if let Err(forbidden) = require(&ctx, "employee", "read_all") {
        return forbidden;
    }
    match recruitment::list_candidates(&state.pool, ctx.tenant_id, id).await {
        Ok(candidates) => Json(json!({ "candidates": candidates })).into_response(),
        Err(e) => map_error(e),
    }
}

/// POST /api/v1/jobs/:id/candidates
pub async fn create_candidate(
    State(state): State<AppState>,
    Authed(ctx): Authed,
    Path(id): Path<Uuid>,
    Json(body): Json<CreateCandidateBody>,
) -> Response {
    if let Err(forbidden) = require(&ctx, "employee", "update") {
        return forbidden;
    }
    let input = CandidateInput {
        first_name: body.first_name,
        last_name: body.last_name,
        email: body.email,
        phone: body.phone,
        source: body.source,
        note: body.note,
    };
    match recruitment::create_candidate(&state.pool, ctx.tenant_id, ctx.user_id.0, id, &input).await
    {
        Ok(candidate) => {
            (StatusCode::CREATED, Json(json!({ "candidate": candidate }))).into_response()
        }
        Err(e) => map_error(e),
    }
}

/// PATCH /api/v1/candidates/:id
pub async fn update_candidate(
    State(state): State<AppState>,
    Authed(ctx): Authed,
    Path(id): Path<Uuid>,
    Json(body): Json<StageBody>,
) -> Response {
    if let Err(forbidden) = require(&ctx, "employee", "update") {
        return forbidden;
    }
    match recruitment::set_candidate_stage(
        &state.pool,
        ctx.tenant_id,
        ctx.user_id.0,
        id,
        &body.stage,
    )
    .await
    {
        Ok(candidate) => Json(json!({ "candidate": candidate })).into_response(),
        Err(e) => map_error(e),
    }
}

/// GET /api/v1/candidates/:id/interviews
pub async fn list_interviews(
    State(state): State<AppState>,
    Authed(ctx): Authed,
    Path(id): Path<Uuid>,
) -> Response {
    if let Err(forbidden) = require(&ctx, "employee", "read_all") {
        return forbidden;
    }
    match recruitment::list_interviews(&state.pool, ctx.tenant_id, id).await {
        Ok(interviews) => Json(json!({ "interviews": interviews })).into_response(),
        Err(e) => map_error(e),
    }
}

/// POST /api/v1/candidates/:id/interviews
pub async fn schedule_interview(
    State(state): State<AppState>,
    Authed(ctx): Authed,
    Path(id): Path<Uuid>,
    Json(body): Json<ScheduleInterviewBody>,
) -> Response {
    if let Err(forbidden) = require(&ctx, "employee", "update") {
        return forbidden;
    }
    let input = InterviewInput {
        scheduled_at: body.scheduled_at,
        mode: body.mode,
        location: body.location,
        interviewer_user_id: body.interviewer_user_id,
        notes: body.notes,
    };
    match recruitment::schedule_interview(&state.pool, ctx.tenant_id, ctx.user_id.0, id, &input)
        .await
    {
        Ok(interview) => {
            (StatusCode::CREATED, Json(json!({ "interview": interview }))).into_response()
        }
        Err(e) => map_error(e),
    }
}

/// PATCH /api/v1/interviews/:id
pub async fn update_interview(
    State(state): State<AppState>,
    Authed(ctx): Authed,
    Path(id): Path<Uuid>,
    Json(body): Json<InterviewStatusBody>,
) -> Response {
    if let Err(forbidden) = require(&ctx, "employee", "update") {
        return forbidden;
    }
    match recruitment::set_interview_status(&state.pool, ctx.tenant_id, id, &body.status).await {
        Ok(interview) => Json(json!({ "interview": interview })).into_response(),
        Err(e) => map_error(e),
    }
}

/// DELETE /api/v1/interviews/:id
pub async fn remove_interview(
    State(state): State<AppState>,
    Authed(ctx): Authed,
    Path(id): Path<Uuid>,
) -> Response {
    if let Err(forbidden) = require(&ctx, "employee", "update") {
        return forbidden;
    }
    match recruitment::delete_interview(&state.pool, ctx.tenant_id, id).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => map_error(e),
    }
}

/// GET /api/v1/candidates/:id/history
pub async fn history(
    State(state): State<AppState>,
    Authed(ctx): Authed,
    Path(id): Path<Uuid>,
) -> Response {
    if let Err(forbidden) = require(&ctx, "employee", "read_all") {
        return forbidden;
    }
    match recruitment::list_stage_history(&state.pool, ctx.tenant_id, id).await {
        Ok(events) => Json(json!({ "history": events })).into_response(),
        Err(e) => map_error(e),
    }
}

/// DELETE /api/v1/candidates/:id
pub async fn remove_candidate(
    State(state): State<AppState>,
    Authed(ctx): Authed,
    Path(id): Path<Uuid>,
) -> Response {
    if let Err(forbidden) = require(&ctx, "employee", "update") {
        return forbidden;
    }
    match recruitment::delete_candidate(&state.pool, ctx.tenant_id, id).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => map_error(e),
    }
}

/// POST /api/v1/candidates/:id/cv
pub async fn upload_cv(
    State(state): State<AppState>,
    Authed(ctx): Authed,
    Path(id): Path<Uuid>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    if let Err(forbidden) = require(&ctx, "employee", "update") {
        return forbidden;
    }
    let content_type = headers
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("application/octet-stream");
    match recruitment::set_cv(&state.pool, ctx.tenant_id, id, content_type, &body).await {
        Ok(()) => Json(json!({ "candidate_id": id.to_string(), "has_cv": true })).into_response(),
        Err(e) => map_error(e),
    }
}

/// GET /api/v1/candidates/:id/cv
pub async fn download_cv(
    State(state): State<AppState>,
    Authed(ctx): Authed,
    Path(id): Path<Uuid>,
) -> Response {
    if let Err(forbidden) = require(&ctx, "employee", "read_all") {
        return forbidden;
    }
    match recruitment::get_cv(&state.pool, ctx.tenant_id, id).await {
        Ok(file) => cv_response(file),
        Err(e) => map_error(e),
    }
}

fn cv_response(file: CandidateFile) -> Response {
    (
        StatusCode::OK,
        [
            (
                header::CONTENT_TYPE,
                HeaderValue::from_str(&file.content_type)
                    .unwrap_or_else(|_| HeaderValue::from_static("application/octet-stream")),
            ),
            (
                header::CONTENT_DISPOSITION,
                HeaderValue::from_static("inline; filename=\"cv\""),
            ),
            (
                header::X_CONTENT_TYPE_OPTIONS,
                HeaderValue::from_static("nosniff"),
            ),
        ],
        file.bytes,
    )
        .into_response()
}

fn map_error(e: RecruitmentError) -> Response {
    use RecruitmentError::*;
    match e {
        JobNotFound | CandidateNotFound | NoCv | InterviewNotFound => (
            StatusCode::NOT_FOUND,
            Json(envelope("not_found", &e.to_string())),
        )
            .into_response(),
        InvalidTitle
        | InvalidEmploymentType
        | InvalidHeadcount
        | InvalidDepartment
        | InvalidDescription
        | InvalidName
        | InvalidEmail
        | InvalidPhone
        | InvalidSource
        | InvalidNote
        | InvalidStage
        | CvTooLarge
        | InvalidCvType
        | InvalidMode
        | InvalidInterviewStatus
        | InvalidLocation
        | InvalidNotes => (
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(envelope("invalid_request", &e.to_string())),
        )
            .into_response(),
        Io(_) | Db(_) => {
            tracing::error!(error = %e, "recruitment endpoint failed");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(envelope("internal_error", "An unexpected error occurred")),
            )
                .into_response()
        }
    }
}

fn invalid(message: &str) -> Response {
    (
        StatusCode::UNPROCESSABLE_ENTITY,
        Json(envelope("invalid_request", message)),
    )
        .into_response()
}

fn envelope(code: &str, message: &str) -> ErrorResponse {
    ErrorResponse {
        error: code.into(),
        message: message.into(),
        details: vec![],
        request_id: Uuid::new_v4().to_string(),
        retry_after_seconds: None,
    }
}
