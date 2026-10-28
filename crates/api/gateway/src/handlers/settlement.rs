//! Final settlement drafts: compute and list.

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use chrono::NaiveDate;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use talentos_hr_runner::settlement::{
    compute_and_store, list_settlements, SettlementError, SettlementInput,
};

use crate::dto::responses::ErrorResponse;
use crate::middleware::auth::{require, Authed};
use crate::state::AppState;

#[derive(Debug, Deserialize)]
pub struct ComputeBody {
    pub termination_date: NaiveDate,
    pub unused_leave_days: i32,
    pub notice_days: i32,
    pub include_severance: bool,
}

#[derive(Debug, Deserialize)]
pub struct ListQuery {
    pub employee_id: Option<Uuid>,
}

#[derive(Debug, Serialize)]
pub struct SettlementDto {
    pub id: Uuid,
    pub employee_id: Uuid,
    pub employee_code: String,
    pub employee_name: String,
    pub termination_date: String,
    pub unused_leave_days: i32,
    pub notice_days: i32,
    pub include_severance: bool,
    pub years_of_service: f64,
    pub last_month_pay: String,
    pub leave_pay: String,
    pub notice_pay: String,
    pub severance_pay: String,
    pub total_gross: String,
    pub breakdown: serde_json::Value,
    pub status: String,
    pub created_at: String,
}

fn view_to_dto(view: talentos_hr_runner::settlement::SettlementView) -> SettlementDto {
    SettlementDto {
        id: view.id,
        employee_id: view.employee_id,
        employee_code: view.employee_code,
        employee_name: view.employee_name,
        termination_date: view.termination_date.to_string(),
        unused_leave_days: view.unused_leave_days,
        notice_days: view.notice_days,
        include_severance: view.include_severance,
        years_of_service: view.years_of_service,
        last_month_pay: view.last_month_pay.to_string(),
        leave_pay: view.leave_pay.to_string(),
        notice_pay: view.notice_pay.to_string(),
        severance_pay: view.severance_pay.to_string(),
        total_gross: view.total_gross.to_string(),
        breakdown: view.breakdown,
        status: view.status,
        created_at: view.created_at.to_rfc3339(),
    }
}

/// POST /api/v1/employees/:id/final-settlement
pub async fn compute(
    State(state): State<AppState>,
    Authed(ctx): Authed,
    Path(id): Path<Uuid>,
    Json(body): Json<ComputeBody>,
) -> Response {
    if let Err(forbidden) = require(&ctx, "employee", "update") {
        return forbidden;
    }
    if let Some(message) = validate(&body) {
        return bad_request(&message);
    }

    let input = SettlementInput {
        termination_date: body.termination_date,
        unused_leave_days: body.unused_leave_days,
        notice_days: body.notice_days,
        include_severance: body.include_severance,
    };

    match compute_and_store(&state.pool, ctx.tenant_id, ctx.user_id.0, id, input).await {
        Ok(view) => (StatusCode::CREATED, Json(view_to_dto(view))).into_response(),
        Err(SettlementError::EmployeeNotFound) => not_found(),
        Err(
            SettlementError::InvalidTerminationDate
            | SettlementError::NegativeDays(_)
            | SettlementError::NegativeSalary,
        ) => bad_request("invalid settlement input"),
        Err(e) => internal_err(&e.to_string()),
    }
}

/// GET /api/v1/final-settlements?employee_id=
pub async fn list(
    State(state): State<AppState>,
    Authed(ctx): Authed,
    Query(query): Query<ListQuery>,
) -> Response {
    if let Err(forbidden) = require(&ctx, "employee", "read_all") {
        return forbidden;
    }

    match list_settlements(&state.pool, ctx.tenant_id, query.employee_id).await {
        Ok(views) => {
            let settlements: Vec<SettlementDto> = views.into_iter().map(view_to_dto).collect();
            (
                StatusCode::OK,
                Json(serde_json::json!({ "settlements": settlements })),
            )
                .into_response()
        }
        Err(e) => internal_err(&e.to_string()),
    }
}

fn validate(body: &ComputeBody) -> Option<String> {
    if body.unused_leave_days < 0 {
        return Some("unused_leave_days must be >= 0".into());
    }
    if body.notice_days < 0 {
        return Some("notice_days must be >= 0".into());
    }
    None
}

fn bad_request(message: &str) -> Response {
    error_response(StatusCode::BAD_REQUEST, "invalid_request", message)
}

fn not_found() -> Response {
    error_response(StatusCode::NOT_FOUND, "not_found", "employee not found")
}

fn internal_err(message: &str) -> Response {
    tracing::error!(error = %message, "settlement endpoint failed");
    error_response(
        StatusCode::INTERNAL_SERVER_ERROR,
        "internal_error",
        "An unexpected error occurred",
    )
}

fn error_response(status: StatusCode, code: &str, message: &str) -> Response {
    (
        status,
        Json(ErrorResponse {
            error: code.into(),
            message: message.into(),
            details: vec![],
            request_id: Uuid::new_v4().to_string(),
            retry_after_seconds: None,
        }),
    )
        .into_response()
}
