//! Approval delegation endpoints: a manager/HR delegates leave-approval to an
//! active user for a date range, lists active delegations, and cancels them.

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;
use uuid::Uuid;

use crate::dto::responses::ErrorResponse;
use crate::middleware::auth::{require, Authed};
use crate::state::AppState;

fn is_hr(ctx: &talentos_kernel::TenantContext) -> bool {
    ctx.roles.iter().any(|r| r == "hr_admin")
}

/// GET /api/v1/approval-delegations — own delegations (HR sees all).
pub async fn list(State(state): State<AppState>, Authed(ctx): Authed) -> Response {
    if let Err(forbidden) = require(&ctx, "approval_delegation", "view_self") {
        return forbidden;
    }
    match talentos_hr_runner::list_delegations(
        &state.pool,
        ctx.tenant_id,
        ctx.user_id.0,
        is_hr(&ctx),
    )
    .await
    {
        Ok(delegations) => {
            (StatusCode::OK, Json(json!({ "delegations": delegations }))).into_response()
        }
        Err(e) => map_error(e),
    }
}

/// GET /api/v1/approval-delegations/candidates — active users available to delegate to.
pub async fn candidates(State(state): State<AppState>, Authed(ctx): Authed) -> Response {
    if let Err(forbidden) = require(&ctx, "approval_delegation", "manage") {
        return forbidden;
    }
    match talentos_hr_runner::list_candidates(&state.pool, ctx.tenant_id).await {
        Ok(candidates) => (StatusCode::OK, Json(json!({ "users": candidates }))).into_response(),
        Err(e) => map_error(e),
    }
}

/// POST /api/v1/approval-delegations
pub async fn create(
    State(state): State<AppState>,
    Authed(ctx): Authed,
    Json(input): Json<talentos_hr_runner::DelegationInput>,
) -> Response {
    if let Err(forbidden) = require(&ctx, "approval_delegation", "manage") {
        return forbidden;
    }
    match talentos_hr_runner::create_delegation(
        &state.pool,
        ctx.tenant_id,
        ctx.user_id.0,
        is_hr(&ctx),
        input,
    )
    .await
    {
        Ok(view) => (StatusCode::CREATED, Json(view)).into_response(),
        Err(e) => map_error(e),
    }
}

/// DELETE /api/v1/approval-delegations/:id
pub async fn delete(
    State(state): State<AppState>,
    Authed(ctx): Authed,
    Path(id): Path<Uuid>,
) -> Response {
    if let Err(forbidden) = require(&ctx, "approval_delegation", "manage") {
        return forbidden;
    }
    match talentos_hr_runner::delete_delegation(
        &state.pool,
        ctx.tenant_id,
        ctx.user_id.0,
        is_hr(&ctx),
        id,
    )
    .await
    {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => map_error(e),
    }
}

fn map_error(e: talentos_hr_runner::DelegationError) -> Response {
    use talentos_hr_runner::DelegationError::*;
    let (status, message) = match e {
        NotFound => (StatusCode::NOT_FOUND, "delegation not found"),
        SelfDelegation => (
            StatusCode::BAD_REQUEST,
            "you cannot delegate approval to yourself",
        ),
        BadRange => (
            StatusCode::BAD_REQUEST,
            "ends_on must be on or after starts_on",
        ),
        InvalidDelegate => (
            StatusCode::BAD_REQUEST,
            "delegate must be an active user in this tenant",
        ),
        InvalidDelegator => (
            StatusCode::BAD_REQUEST,
            "delegator must be an active user in this tenant",
        ),
        Duplicate => (
            StatusCode::CONFLICT,
            "an identical delegation already exists",
        ),
        Db(_) => {
            return internal_err(&e.to_string());
        }
    };
    (
        status,
        Json(ErrorResponse {
            error: if status == StatusCode::CONFLICT {
                "conflict".into()
            } else {
                "invalid_request".into()
            },
            message: message.into(),
            details: vec![],
            request_id: Uuid::new_v4().to_string(),
            retry_after_seconds: None,
        }),
    )
        .into_response()
}

fn internal_err(message: &str) -> Response {
    tracing::error!(error = %message, "delegation endpoint failed");
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(ErrorResponse {
            error: "internal_error".into(),
            message: "An unexpected error occurred".into(),
            details: vec![],
            request_id: Uuid::new_v4().to_string(),
            retry_after_seconds: None,
        }),
    )
        .into_response()
}
