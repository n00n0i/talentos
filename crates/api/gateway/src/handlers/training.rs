//! Employee training history and expiring certificates.

use axum::{
    extract::{Path, Query, State},
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
use talentos_hr_runner::training::{self, TrainingError, TrainingInput};

#[derive(Debug, Deserialize)]
pub struct CreateTrainingBody {
    pub title: String,
    pub provider: Option<String>,
    pub completed_on: Option<NaiveDate>,
    pub expires_on: Option<NaiveDate>,
}

#[derive(Debug, Deserialize)]
pub struct ExpiringQuery {
    pub days: Option<i64>,
}

/// GET /api/v1/employees/:id/training
pub async fn list(
    State(state): State<AppState>,
    Authed(ctx): Authed,
    Path(id): Path<Uuid>,
) -> Response {
    if let Err(forbidden) = require(&ctx, "employee", "read_all") {
        return forbidden;
    }
    match training::list_training(&state.pool, ctx.tenant_id, id).await {
        Ok(records) => Json(json!({ "training": records })).into_response(),
        Err(e) => map_error(e),
    }
}

/// POST /api/v1/employees/:id/training
pub async fn create(
    State(state): State<AppState>,
    Authed(ctx): Authed,
    Path(id): Path<Uuid>,
    Json(body): Json<CreateTrainingBody>,
) -> Response {
    if let Err(forbidden) = require(&ctx, "employee", "update") {
        return forbidden;
    }
    let input = TrainingInput {
        title: body.title,
        provider: body.provider,
        completed_on: body.completed_on,
        expires_on: body.expires_on,
    };
    match training::create_training(&state.pool, ctx.tenant_id, ctx.user_id.0, id, &input).await {
        Ok(record) => (StatusCode::CREATED, Json(json!({ "training": record }))).into_response(),
        Err(e) => map_error(e),
    }
}

/// DELETE /api/v1/training/:id
pub async fn remove(
    State(state): State<AppState>,
    Authed(ctx): Authed,
    Path(id): Path<Uuid>,
) -> Response {
    if let Err(forbidden) = require(&ctx, "employee", "update") {
        return forbidden;
    }
    match training::delete_training(&state.pool, ctx.tenant_id, id).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => map_error(e),
    }
}

/// GET /api/v1/training/expiring?days=90
pub async fn expiring(
    State(state): State<AppState>,
    Authed(ctx): Authed,
    Query(query): Query<ExpiringQuery>,
) -> Response {
    if let Err(forbidden) = require(&ctx, "employee", "read_all") {
        return forbidden;
    }
    match training::expiring_training(&state.pool, ctx.tenant_id, query.days.unwrap_or(90)).await {
        Ok(records) => Json(json!({ "training": records })).into_response(),
        Err(e) => map_error(e),
    }
}

/// GET /api/v1/me/training
pub async fn me_list(State(state): State<AppState>, Authed(ctx): Authed) -> Response {
    match training::list_my_training(&state.pool, ctx.tenant_id, ctx.user_id.0).await {
        Ok(records) => Json(json!({ "training": records })).into_response(),
        Err(e) => map_error(e),
    }
}

fn map_error(e: TrainingError) -> Response {
    use TrainingError::*;
    match e {
        EmployeeNotFound | NotFound => (
            StatusCode::NOT_FOUND,
            Json(envelope("not_found", &e.to_string())),
        )
            .into_response(),
        InvalidTitle | InvalidProvider | InvalidDates => (
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(envelope("invalid_request", &e.to_string())),
        )
            .into_response(),
        Db(_) => {
            tracing::error!(error = %e, "training endpoint failed");
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
