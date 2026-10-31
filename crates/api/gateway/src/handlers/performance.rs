//! Performance review cycles and reviews.

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
use talentos_hr_runner::performance::{self, CycleInput, PerformanceError};

#[derive(Debug, Deserialize)]
pub struct CreateCycleBody {
    pub name: String,
    pub period_start: NaiveDate,
    pub period_end: NaiveDate,
}

#[derive(Debug, Deserialize)]
pub struct SubmitReviewBody {
    pub rating: i32,
    pub strengths: Option<String>,
    pub improvements: Option<String>,
}

/// GET /api/v1/performance/cycles
pub async fn list_cycles(State(state): State<AppState>, Authed(ctx): Authed) -> Response {
    if let Err(forbidden) = require(&ctx, "employee", "read_all") {
        return forbidden;
    }
    match performance::list_cycles(&state.pool, ctx.tenant_id).await {
        Ok(cycles) => Json(json!({ "cycles": cycles })).into_response(),
        Err(e) => map_error(e),
    }
}

/// POST /api/v1/performance/cycles
pub async fn create_cycle(
    State(state): State<AppState>,
    Authed(ctx): Authed,
    Json(body): Json<CreateCycleBody>,
) -> Response {
    if let Err(forbidden) = require(&ctx, "employee", "update") {
        return forbidden;
    }
    let input = CycleInput {
        name: body.name,
        period_start: body.period_start,
        period_end: body.period_end,
    };
    match performance::create_cycle(&state.pool, ctx.tenant_id, ctx.user_id.0, &input).await {
        Ok(cycle) => (StatusCode::CREATED, Json(json!({ "cycle": cycle }))).into_response(),
        Err(e) => map_error(e),
    }
}

/// POST /api/v1/performance/cycles/:id/start
pub async fn start_cycle(
    State(state): State<AppState>,
    Authed(ctx): Authed,
    Path(id): Path<Uuid>,
) -> Response {
    if let Err(forbidden) = require(&ctx, "employee", "update") {
        return forbidden;
    }
    match performance::start_cycle(&state.pool, ctx.tenant_id, id).await {
        Ok(()) => Json(json!({ "id": id.to_string(), "status": "active" })).into_response(),
        Err(e) => map_error(e),
    }
}

/// POST /api/v1/performance/cycles/:id/close
pub async fn close_cycle(
    State(state): State<AppState>,
    Authed(ctx): Authed,
    Path(id): Path<Uuid>,
) -> Response {
    if let Err(forbidden) = require(&ctx, "employee", "update") {
        return forbidden;
    }
    match performance::close_cycle(&state.pool, ctx.tenant_id, id).await {
        Ok(()) => Json(json!({ "id": id.to_string(), "status": "closed" })).into_response(),
        Err(e) => map_error(e),
    }
}

/// DELETE /api/v1/performance/cycles/:id
pub async fn remove_cycle(
    State(state): State<AppState>,
    Authed(ctx): Authed,
    Path(id): Path<Uuid>,
) -> Response {
    if let Err(forbidden) = require(&ctx, "employee", "update") {
        return forbidden;
    }
    match performance::delete_cycle(&state.pool, ctx.tenant_id, id).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => map_error(e),
    }
}

/// GET /api/v1/performance/cycles/:id/reviews
pub async fn list_reviews(
    State(state): State<AppState>,
    Authed(ctx): Authed,
    Path(id): Path<Uuid>,
) -> Response {
    if let Err(forbidden) = require(&ctx, "employee", "read_all") {
        return forbidden;
    }
    match performance::list_reviews(&state.pool, ctx.tenant_id, id).await {
        Ok(reviews) => Json(json!({ "reviews": reviews })).into_response(),
        Err(e) => map_error(e),
    }
}

/// PATCH /api/v1/performance/reviews/:id
pub async fn submit_review(
    State(state): State<AppState>,
    Authed(ctx): Authed,
    Path(id): Path<Uuid>,
    Json(body): Json<SubmitReviewBody>,
) -> Response {
    if let Err(forbidden) = require(&ctx, "employee", "update") {
        return forbidden;
    }
    match performance::submit_review(
        &state.pool,
        ctx.tenant_id,
        ctx.user_id.0,
        id,
        body.rating,
        body.strengths.as_deref(),
        body.improvements.as_deref(),
    )
    .await
    {
        Ok(review) => Json(json!({ "review": review })).into_response(),
        Err(e) => map_error(e),
    }
}

/// POST /api/v1/me/reviews/:id/acknowledge
pub async fn me_acknowledge(
    State(state): State<AppState>,
    Authed(ctx): Authed,
    Path(id): Path<Uuid>,
) -> Response {
    match performance::acknowledge_review(&state.pool, ctx.tenant_id, ctx.user_id.0, id).await {
        Ok(review) => Json(json!({ "review": review })).into_response(),
        Err(e) => map_error(e),
    }
}

/// GET /api/v1/me/reviews
pub async fn me_reviews(State(state): State<AppState>, Authed(ctx): Authed) -> Response {
    match performance::list_my_reviews(&state.pool, ctx.tenant_id, ctx.user_id.0).await {
        Ok(reviews) => Json(json!({ "reviews": reviews })).into_response(),
        Err(e) => map_error(e),
    }
}

fn map_error(e: PerformanceError) -> Response {
    use PerformanceError::*;
    match e {
        CycleNotFound | ReviewNotFound => (
            StatusCode::NOT_FOUND,
            Json(envelope("not_found", &e.to_string())),
        )
            .into_response(),
        CycleNotDraft | CycleNotActive | ReviewNotSubmitted => (
            StatusCode::CONFLICT,
            Json(envelope("conflict", &e.to_string())),
        )
            .into_response(),
        InvalidName | InvalidPeriod | InvalidRating | InvalidText => (
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(envelope("invalid_request", &e.to_string())),
        )
            .into_response(),
        Db(_) => {
            tracing::error!(error = %e, "performance endpoint failed");
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
