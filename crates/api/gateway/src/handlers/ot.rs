//! OT (overtime) endpoints: self-service requests and approval decisions.

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

#[derive(Debug, Deserialize)]
pub struct DecideOtBody {
    pub approve: bool,
}

#[derive(Debug, Deserialize)]
pub struct CreateOtBody {
    pub work_date: NaiveDate,
    pub minutes: i32,
    pub reason: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ApprovalsQuery {
    pub status: Option<String>,
    pub from: Option<NaiveDate>,
    pub to: Option<NaiveDate>,
    pub limit: Option<i64>,
}

/// GET /api/v1/me/ot
pub async fn me_ot(State(state): State<AppState>, Authed(ctx): Authed) -> Response {
    if let Err(forbidden) = require(&ctx, "ot_request", "view_self") {
        return forbidden;
    }
    let employee_id =
        match talentos_hr_runner::employee_of_user(&state.pool, ctx.tenant_id, ctx.user_id.0).await
        {
            Ok(Some(id)) => id,
            Ok(None) => {
                return (
                    StatusCode::OK,
                    Json(json!({ "employee_linked": false, "requests": [] })),
                )
                    .into_response()
            }
            Err(e) => return internal_err(&e.to_string()),
        };

    let requests =
        match talentos_hr_runner::list_ot_requests(&state.pool, ctx.tenant_id, employee_id, None)
            .await
        {
            Ok(list) => list,
            Err(e) => return map_ot_error(e),
        };

    (
        StatusCode::OK,
        Json(json!({ "employee_linked": true, "requests": requests })),
    )
        .into_response()
}

/// POST /api/v1/me/ot
pub async fn create_me_ot(
    State(state): State<AppState>,
    Authed(ctx): Authed,
    Json(body): Json<CreateOtBody>,
) -> Response {
    if let Err(forbidden) = require(&ctx, "ot_request", "submit_self") {
        return forbidden;
    }
    let input = talentos_hr_runner::OtInput {
        work_date: body.work_date,
        minutes: body.minutes,
        reason: body.reason,
    };
    match talentos_hr_runner::create_ot_request(&state.pool, ctx.tenant_id, ctx.user_id.0, input)
        .await
    {
        Ok(view) => (StatusCode::CREATED, Json(view)).into_response(),
        Err(e) => map_ot_error(e),
    }
}

/// POST /api/v1/me/ot/:id/cancel
pub async fn me_cancel(
    State(state): State<AppState>,
    Authed(ctx): Authed,
    Path(id): Path<Uuid>,
) -> Response {
    if let Err(forbidden) = require(&ctx, "ot_request", "submit_self") {
        return forbidden;
    }
    match talentos_hr_runner::cancel_ot_request(&state.pool, ctx.tenant_id, ctx.user_id.0, id).await
    {
        Ok(()) => (
            StatusCode::OK,
            Json(json!({ "request_id": id.to_string(), "status": "cancelled" })),
        )
            .into_response(),
        Err(e) => map_ot_error(e),
    }
}

/// GET /api/v1/ot/approvals
pub async fn approvals(
    State(state): State<AppState>,
    Authed(ctx): Authed,
    Query(q): Query<ApprovalsQuery>,
) -> Response {
    if let Err(forbidden) = require(&ctx, "ot_request", "view_team") {
        return forbidden;
    }
    // Default to the pending inbox; `?status=approved` serves the audit trail.
    let status = q.status.as_deref().or(Some("pending"));
    let limit = q.limit.unwrap_or(100);
    match talentos_hr_runner::ot_tenant_requests(
        &state.pool,
        ctx.tenant_id,
        status,
        q.from,
        q.to,
        limit,
    )
    .await
    {
        Ok(requests) => (StatusCode::OK, Json(json!({ "requests": requests }))).into_response(),
        Err(e) => map_ot_error(e),
    }
}

/// POST /api/v1/ot/requests/:id/decision
pub async fn decide(
    State(state): State<AppState>,
    Authed(ctx): Authed,
    Path(id): Path<Uuid>,
    Json(body): Json<DecideOtBody>,
) -> Response {
    if let Err(forbidden) = require(&ctx, "ot_request", "approve_team") {
        return forbidden;
    }
    match talentos_hr_runner::decide_ot_request(
        &state.pool,
        ctx.tenant_id,
        ctx.user_id.0,
        id,
        body.approve,
    )
    .await
    {
        Ok(status) => (
            StatusCode::OK,
            Json(json!({
                "request_id": id.to_string(),
                "status": status,
                "decided_by": ctx.user_id.to_string(),
            })),
        )
            .into_response(),
        Err(e) => map_ot_error(e),
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

fn conflict(message: &str) -> Response {
    (StatusCode::CONFLICT, Json(envelope("conflict", message))).into_response()
}

fn unprocessable(message: &str) -> Response {
    (
        StatusCode::UNPROCESSABLE_ENTITY,
        Json(envelope("invalid_request", message)),
    )
        .into_response()
}

fn map_ot_error(e: talentos_hr_runner::OtError) -> Response {
    match e {
        talentos_hr_runner::OtError::NotFound => not_found(),
        talentos_hr_runner::OtError::NotPending => {
            conflict("OT request cannot be changed in its current status")
        }
        talentos_hr_runner::OtError::DuplicateDate => {
            conflict("a request already exists for this work date")
        }
        talentos_hr_runner::OtError::DayAlreadyApproved => {
            conflict("attendance day is already approved; revoke approval before changing OT")
        }
        talentos_hr_runner::OtError::InvalidMinutes
        | talentos_hr_runner::OtError::InvalidReason
        | talentos_hr_runner::OtError::InvalidStatus => unprocessable(&e.to_string()),
        talentos_hr_runner::OtError::EmployeeNotFound => (
            StatusCode::FORBIDDEN,
            Json(envelope(
                "forbidden",
                "no active employee record for this account",
            )),
        )
            .into_response(),
        _ => internal_err(&e.to_string()),
    }
}

fn not_found() -> Response {
    (
        StatusCode::NOT_FOUND,
        Json(envelope("not_found", "not found")),
    )
        .into_response()
}

fn internal_err(message: &str) -> Response {
    tracing::error!(error = %message, "ot endpoint failed");
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(envelope("internal_error", "An unexpected error occurred")),
    )
        .into_response()
}
