//! Performance goals/OKRs.

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

use crate::dto::responses::ErrorResponse;
use crate::middleware::auth::{require, Authed};
use crate::state::AppState;
use talentos_hr_runner::goals::{self, GoalError, GoalInput};

#[derive(Debug, Deserialize)]
pub struct CreateGoalBody {
    pub cycle_id: Option<Uuid>,
    pub title: String,
    pub description: Option<String>,
    pub target: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateGoalBody {
    pub progress: i32,
    pub status: String,
}

#[derive(Debug, Deserialize)]
pub struct CycleQuery {
    pub cycle_id: Option<Uuid>,
}

/// GET /api/v1/employees/:id/goals?cycle_id=
pub async fn list(
    State(state): State<AppState>,
    Authed(ctx): Authed,
    Path(id): Path<Uuid>,
    Query(query): Query<CycleQuery>,
) -> Response {
    if let Err(forbidden) = require(&ctx, "employee", "read_all") {
        return forbidden;
    }
    match goals::list_goals(&state.pool, ctx.tenant_id, id, query.cycle_id).await {
        Ok(goals) => Json(json!({ "goals": goals })).into_response(),
        Err(e) => map_error(e),
    }
}

/// POST /api/v1/employees/:id/goals
pub async fn create(
    State(state): State<AppState>,
    Authed(ctx): Authed,
    Path(id): Path<Uuid>,
    Json(body): Json<CreateGoalBody>,
) -> Response {
    if let Err(forbidden) = require(&ctx, "employee", "update") {
        return forbidden;
    }
    let input = GoalInput {
        cycle_id: body.cycle_id,
        title: body.title,
        description: body.description,
        target: body.target,
    };
    match goals::create_goal(&state.pool, ctx.tenant_id, ctx.user_id.0, id, &input).await {
        Ok(goal) => (StatusCode::CREATED, Json(json!({ "goal": goal }))).into_response(),
        Err(e) => map_error(e),
    }
}

/// PATCH /api/v1/goals/:id
pub async fn update(
    State(state): State<AppState>,
    Authed(ctx): Authed,
    Path(id): Path<Uuid>,
    Json(body): Json<UpdateGoalBody>,
) -> Response {
    if let Err(forbidden) = require(&ctx, "employee", "update") {
        return forbidden;
    }
    match goals::update_goal(&state.pool, ctx.tenant_id, id, body.progress, &body.status).await {
        Ok(goal) => Json(json!({ "goal": goal })).into_response(),
        Err(e) => map_error(e),
    }
}

/// DELETE /api/v1/goals/:id
pub async fn remove(
    State(state): State<AppState>,
    Authed(ctx): Authed,
    Path(id): Path<Uuid>,
) -> Response {
    if let Err(forbidden) = require(&ctx, "employee", "update") {
        return forbidden;
    }
    match goals::delete_goal(&state.pool, ctx.tenant_id, id).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => map_error(e),
    }
}

/// GET /api/v1/me/goals
pub async fn me_list(State(state): State<AppState>, Authed(ctx): Authed) -> Response {
    match goals::list_my_goals(&state.pool, ctx.tenant_id, ctx.user_id.0).await {
        Ok(goals) => Json(json!({ "goals": goals })).into_response(),
        Err(e) => map_error(e),
    }
}

fn map_error(e: GoalError) -> Response {
    use GoalError::*;
    match e {
        EmployeeNotFound | CycleNotFound | GoalNotFound => (
            StatusCode::NOT_FOUND,
            Json(envelope("not_found", &e.to_string())),
        )
            .into_response(),
        InvalidTitle | InvalidDescription | InvalidTarget | InvalidProgress | InvalidStatus => (
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(envelope("invalid_request", &e.to_string())),
        )
            .into_response(),
        Db(_) => {
            tracing::error!(error = %e, "goal endpoint failed");
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
