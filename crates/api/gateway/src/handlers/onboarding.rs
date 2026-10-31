//! Employee onboarding checklist: HR management and self-service.

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use chrono::NaiveDate;
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

use crate::dto::responses::ErrorResponse;
use crate::middleware::auth::{require, Authed};
use crate::state::AppState;
use talentos_hr_runner::onboarding::{self, OnboardingError, TaskInput};

#[derive(Debug, Deserialize)]
pub struct CreateTaskBody {
    pub title: String,
    pub notes: Option<String>,
    pub due_date: Option<NaiveDate>,
}

#[derive(Debug, Deserialize)]
pub struct DoneBody {
    pub done: bool,
}

/// GET /api/v1/employees/:id/onboarding
pub async fn list(
    State(state): State<AppState>,
    Authed(ctx): Authed,
    Path(id): Path<Uuid>,
) -> Response {
    if let Err(forbidden) = require(&ctx, "employee", "read_all") {
        return forbidden;
    }
    match onboarding::list_tasks(&state.pool, ctx.tenant_id, id).await {
        Ok(tasks) => Json(json!({ "tasks": tasks })).into_response(),
        Err(e) => map_error(e),
    }
}

/// POST /api/v1/employees/:id/onboarding
pub async fn create(
    State(state): State<AppState>,
    Authed(ctx): Authed,
    Path(id): Path<Uuid>,
    Json(body): Json<CreateTaskBody>,
) -> Response {
    if let Err(forbidden) = require(&ctx, "employee", "update") {
        return forbidden;
    }
    let input = TaskInput {
        title: body.title,
        notes: body.notes,
        due_date: body.due_date,
    };
    match onboarding::create_task(&state.pool, ctx.tenant_id, ctx.user_id.0, id, &input).await {
        Ok(task) => (StatusCode::CREATED, Json(json!({ "task": task }))).into_response(),
        Err(e) => map_error(e),
    }
}

/// POST /api/v1/employees/:id/onboarding/standard
pub async fn standard(
    State(state): State<AppState>,
    Authed(ctx): Authed,
    Path(id): Path<Uuid>,
) -> Response {
    if let Err(forbidden) = require(&ctx, "employee", "update") {
        return forbidden;
    }
    match onboarding::start_standard(&state.pool, ctx.tenant_id, ctx.user_id.0, id).await {
        Ok(tasks) => Json(json!({ "tasks": tasks })).into_response(),
        Err(e) => map_error(e),
    }
}

/// PATCH /api/v1/onboarding/:id
pub async fn update(
    State(state): State<AppState>,
    Authed(ctx): Authed,
    Path(id): Path<Uuid>,
    Json(body): Json<DoneBody>,
) -> Response {
    if let Err(forbidden) = require(&ctx, "employee", "update") {
        return forbidden;
    }
    match onboarding::set_done(&state.pool, ctx.tenant_id, ctx.user_id.0, id, body.done).await {
        Ok(task) => Json(json!({ "task": task })).into_response(),
        Err(e) => map_error(e),
    }
}

/// DELETE /api/v1/onboarding/:id
pub async fn remove(
    State(state): State<AppState>,
    Authed(ctx): Authed,
    Path(id): Path<Uuid>,
) -> Response {
    if let Err(forbidden) = require(&ctx, "employee", "update") {
        return forbidden;
    }
    match onboarding::delete_task(&state.pool, ctx.tenant_id, id).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => map_error(e),
    }
}

/// GET /api/v1/me/onboarding
pub async fn me_list(State(state): State<AppState>, Authed(ctx): Authed) -> Response {
    match onboarding::list_my_tasks(&state.pool, ctx.tenant_id, ctx.user_id.0).await {
        Ok(tasks) => Json(json!({ "tasks": tasks })).into_response(),
        Err(e) => map_error(e),
    }
}

/// PATCH /api/v1/me/onboarding/:id
pub async fn me_update(
    State(state): State<AppState>,
    Authed(ctx): Authed,
    Path(id): Path<Uuid>,
    Json(body): Json<DoneBody>,
) -> Response {
    match onboarding::set_my_done(&state.pool, ctx.tenant_id, ctx.user_id.0, id, body.done).await {
        Ok(task) => Json(json!({ "task": task })).into_response(),
        Err(e) => map_error(e),
    }
}

fn map_error(e: OnboardingError) -> Response {
    use OnboardingError::*;
    match e {
        EmployeeNotFound | NotFound => (
            StatusCode::NOT_FOUND,
            Json(envelope("not_found", &e.to_string())),
        )
            .into_response(),
        DuplicateTask => (
            StatusCode::CONFLICT,
            Json(envelope("duplicate_task", &e.to_string())),
        )
            .into_response(),
        InvalidTitle | InvalidNotes => (
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(envelope("invalid_request", &e.to_string())),
        )
            .into_response(),
        Db(_) => {
            tracing::error!(error = %e, "onboarding endpoint failed");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(envelope("internal_error", "An unexpected error occurred")),
            )
                .into_response()
        }
    }
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
