//! Leave endpoints: self-service requests, balances, and approval decisions.

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use chrono::{Datelike, NaiveDate};
use serde::Deserialize;
use serde_json::json;
use sqlx::Row;
use uuid::Uuid;

use crate::dto::responses::ErrorResponse;
use crate::middleware::auth::{require, Authed};
use crate::state::AppState;

#[derive(Debug, Deserialize)]
pub struct ApproveLeaveRequest {
    pub approve: bool,
    /// Optional note; persisted with the approval audit entry.
    #[allow(dead_code)]
    pub reason: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct CreateLeaveBody {
    pub leave_type: String,
    pub start_date: NaiveDate,
    pub end_date: NaiveDate,
    pub reason: Option<String>,
    pub employee_id: Option<Uuid>,
}

#[derive(Debug, Deserialize)]
pub struct EmployeeQuery {
    pub employee_id: Option<Uuid>,
    pub year: Option<i32>,
    pub status: Option<String>,
}

/// POST /api/v1/leave/requests — create a request for self (or a given employee).
pub async fn create(
    State(state): State<AppState>,
    Authed(ctx): Authed,
    Json(body): Json<CreateLeaveBody>,
) -> Response {
    if let Err(forbidden) = require(&ctx, "leave_request", "submit_self") {
        return forbidden;
    }
    let input = talentos_hr_runner::LeaveInput {
        leave_type: body.leave_type,
        start_date: body.start_date,
        end_date: body.end_date,
        reason: body.reason,
        employee_id: body.employee_id,
    };

    match talentos_hr_runner::create_request(&state.pool, ctx.tenant_id, ctx.user_id.0, input).await
    {
        Ok(view) => (StatusCode::CREATED, Json(view)).into_response(),
        Err(e) => map_leave_error(e),
    }
}

/// GET /api/v1/leave/balances?employee_id=&year=
pub async fn balances(
    State(state): State<AppState>,
    Authed(ctx): Authed,
    Query(q): Query<EmployeeQuery>,
) -> Response {
    if let Err(forbidden) = require(&ctx, "leave_request", "view_team") {
        return forbidden;
    }
    let employee_id =
        match resolve_employee(&state, ctx.tenant_id, ctx.user_id.0, q.employee_id).await {
            Ok(Some(id)) => id,
            Ok(None) => return not_found(),
            Err(e) => return internal_err(&e),
        };
    let year = q.year.unwrap_or_else(|| chrono::Utc::now().year());

    match talentos_hr_runner::list_balances(&state.pool, ctx.tenant_id, employee_id, year).await {
        Ok(list) => (StatusCode::OK, Json(list)).into_response(),
        Err(e) => map_leave_error(e),
    }
}

/// GET /api/v1/leave/requests?employee_id=&status=
pub async fn list(
    State(state): State<AppState>,
    Authed(ctx): Authed,
    Query(q): Query<EmployeeQuery>,
) -> Response {
    if let Err(forbidden) = require(&ctx, "leave_request", "view_team") {
        return forbidden;
    }
    let employee_id =
        match resolve_employee(&state, ctx.tenant_id, ctx.user_id.0, q.employee_id).await {
            Ok(Some(id)) => id,
            Ok(None) => return not_found(),
            Err(e) => return internal_err(&e),
        };

    match talentos_hr_runner::list_requests(
        &state.pool,
        ctx.tenant_id,
        employee_id,
        q.status.as_deref(),
    )
    .await
    {
        Ok(list) => (StatusCode::OK, Json(list)).into_response(),
        Err(e) => map_leave_error(e),
    }
}

/// GET /api/v1/me/leave
pub async fn me_leave(State(state): State<AppState>, Authed(ctx): Authed) -> Response {
    if let Err(forbidden) = require(&ctx, "leave_request", "view_self") {
        return forbidden;
    }
    let employee_id =
        match talentos_hr_runner::employee_of_user(&state.pool, ctx.tenant_id, ctx.user_id.0).await
        {
            Ok(Some(id)) => id,
            Ok(None) => {
                return (
                    StatusCode::OK,
                    Json(json!({ "employee_linked": false, "balances": [], "requests": [] })),
                )
                    .into_response()
            }
            Err(e) => return internal_err(&e.to_string()),
        };

    let year = chrono::Utc::now().year();
    let balances =
        match talentos_hr_runner::list_balances(&state.pool, ctx.tenant_id, employee_id, year).await
        {
            Ok(list) => list,
            Err(e) => return map_leave_error(e),
        };
    let requests =
        match talentos_hr_runner::list_requests(&state.pool, ctx.tenant_id, employee_id, None).await
        {
            Ok(list) => list,
            Err(e) => return map_leave_error(e),
        };

    (
        StatusCode::OK,
        Json(json!({
            "employee_linked": true,
            "balances": balances,
            "requests": requests,
        })),
    )
        .into_response()
}

/// POST /api/v1/me/leave/:id/cancel
pub async fn me_cancel(
    State(state): State<AppState>,
    Authed(ctx): Authed,
    Path(id): Path<Uuid>,
) -> Response {
    if let Err(forbidden) = require(&ctx, "leave_request", "submit_self") {
        return forbidden;
    }
    match talentos_hr_runner::cancel_request(&state.pool, ctx.tenant_id, ctx.user_id.0, id).await {
        Ok(status) => (
            StatusCode::OK,
            Json(json!({ "request_id": id.to_string(), "status": status })),
        )
            .into_response(),
        Err(e) => map_leave_error(e),
    }
}

/// GET /api/v1/leave/approvals
pub async fn approvals(State(state): State<AppState>, Authed(ctx): Authed) -> Response {
    if let Err(forbidden) = require(&ctx, "leave_request", "view_team") {
        return forbidden;
    }
    match talentos_hr_runner::pending_approvals(&state.pool, ctx.tenant_id).await {
        Ok(requests) => (
            StatusCode::OK,
            Json(json!({ "requests": requests })),
        )
            .into_response(),
        Err(e) => map_leave_error(e),
    }
}

async fn resolve_employee(
    state: &AppState,
    tenant_id: talentos_kernel::TenantId,
    user_id: Uuid,
    requested: Option<Uuid>,
) -> Result<Option<Uuid>, String> {
    if requested.is_some() {
        return Ok(requested);
    }
    talentos_hr_runner::employee_of_user(&state.pool, tenant_id, user_id)
        .await
        .map_err(|e| e.to_string())
}

/// POST /api/v1/leave/requests/:id/decision
pub async fn decide(
    State(state): State<AppState>,
    Authed(ctx): Authed,
    Path(id): Path<Uuid>,
    Json(body): Json<ApproveLeaveRequest>,
) -> Response {
    if let Err(forbidden) = require(&ctx, "leave_request", "approve_team") {
        return forbidden;
    }
    let mut tx = match state.pool.begin_tenant(ctx.tenant_id).await {
        Ok(tx) => tx,
        Err(e) => return internal_err(&e.to_string()),
    };

    // Only pending requests may be decided; the guard is in SQL so two approvers
    // cannot both "win" a race.
    let updated = sqlx::query(
        r#"UPDATE leave_requests
           SET status = $3, approver_id = $4, approved_at = now(), updated_at = now()
           WHERE id = $1 AND tenant_id = $2 AND status = 'pending'
           RETURNING status, employee_id, leave_type, start_date, days"#,
    )
    .bind(id)
    .bind(ctx.tenant_id.0)
    .bind(if body.approve { "approved" } else { "rejected" })
    .bind(ctx.user_id.0)
    .fetch_optional(&mut *tx)
    .await;

    let row = match updated {
        Ok(Some(row)) => row,
        Ok(None) => {
            let _ = tx.rollback().await;
            return not_found();
        }
        Err(e) => return internal_err(&e.to_string()),
    };

    let status: String = match row.try_get("status") {
        Ok(v) => v,
        Err(e) => return internal_err(&e.to_string()),
    };

    // Rejection restores the balance that was debited when the request was created.
    if !body.approve {
        let employee_id: Uuid = match row.try_get("employee_id") {
            Ok(v) => v,
            Err(e) => return internal_err(&e.to_string()),
        };
        let leave_type: String = match row.try_get("leave_type") {
            Ok(v) => v,
            Err(e) => return internal_err(&e.to_string()),
        };
        let year: i32 = match row.try_get::<chrono::NaiveDate, _>("start_date") {
            Ok(v) => v.year(),
            Err(e) => return internal_err(&e.to_string()),
        };
        let days: rust_decimal::Decimal = match row.try_get("days") {
            Ok(v) => v,
            Err(e) => return internal_err(&e.to_string()),
        };

        if leave_type != "unpaid" {
            if let Err(e) = sqlx::query(
                r#"UPDATE leave_balances lb
                   SET used_days = used_days - $1, updated_at = now()
                   FROM leave_types lt
                   WHERE lb.employee_id = $2
                     AND lb.year = $3
                     AND lt.id = lb.leave_type_id
                     AND lt.name = $4
                     AND (lt.tenant_id IS NULL OR lt.tenant_id = $5)"#,
            )
            .bind(days)
            .bind(employee_id)
            .bind(year)
            .bind(&leave_type)
            .bind(ctx.tenant_id.0)
            .execute(&mut *tx)
            .await
            {
                let _ = tx.rollback().await;
                return internal_err(&e.to_string());
            }
        }
    }

    if let Err(e) = tx.commit().await {
        return internal_err(&e.to_string());
    }

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "request_id": id.to_string(),
            "status": status,
            "approver_id": ctx.user_id.to_string(),
        })),
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

fn unprocessable(message: &str) -> Response {
    (
        StatusCode::UNPROCESSABLE_ENTITY,
        Json(envelope("invalid_request", message)),
    )
        .into_response()
}

fn conflict(message: &str) -> Response {
    (StatusCode::CONFLICT, Json(envelope("conflict", message))).into_response()
}

fn map_leave_error(e: talentos_hr_runner::LeaveError) -> Response {
    match e {
        talentos_hr_runner::LeaveError::NotFound => not_found(),
        talentos_hr_runner::LeaveError::NotCancellable => {
            conflict("leave request cannot be cancelled in its current status")
        }
        talentos_hr_runner::LeaveError::Overlap(s, e) => {
            conflict(&format!("overlaps an existing leave from {s} to {e}"))
        }
        talentos_hr_runner::LeaveError::InsufficientBalance { .. } => unprocessable(&e.to_string()),
        talentos_hr_runner::LeaveError::UnknownType
        | talentos_hr_runner::LeaveError::BadRange
        | talentos_hr_runner::LeaveError::TooLong => unprocessable(&e.to_string()),
        talentos_hr_runner::LeaveError::EmployeeNotFound | talentos_hr_runner::LeaveError::NotActive => (
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
    tracing::error!(error = %message, "leave endpoint failed");
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(envelope("internal_error", "An unexpected error occurred")),
    )
        .into_response()
}
