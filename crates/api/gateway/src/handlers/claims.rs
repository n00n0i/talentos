//! Expense claim endpoints: self-service submission, receipt upload/download,
//! and HR/manager approval.

use axum::{
    body::Bytes,
    extract::{Path, State},
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use chrono::NaiveDate;
use rust_decimal::Decimal;
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

use crate::dto::responses::ErrorResponse;
use crate::middleware::auth::{require, Authed};
use crate::state::AppState;

#[derive(Debug, Deserialize)]
pub struct CreateClaimBody {
    pub category: String,
    pub amount: Decimal,
    pub expense_date: NaiveDate,
    pub description: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct DecideClaimBody {
    pub approve: bool,
}

/// GET /api/v1/me/claims
pub async fn me_claims(State(state): State<AppState>, Authed(ctx): Authed) -> Response {
    if let Err(forbidden) = require(&ctx, "claim", "submit_self") {
        return forbidden;
    }
    let employee_id =
        match talentos_hr_runner::claim_employee_of_user(&state.pool, ctx.tenant_id, ctx.user_id.0)
            .await
        {
            Ok(Some(id)) => id,
            Ok(None) => {
                return (
                    StatusCode::OK,
                    Json(json!({ "employee_linked": false, "claims": [] })),
                )
                    .into_response()
            }
            Err(e) => return internal_err(&e.to_string()),
        };

    let claims = match talentos_hr_runner::list_claims(
        &state.pool,
        ctx.tenant_id,
        employee_id,
        None,
    )
    .await
    {
        Ok(list) => list,
        Err(e) => return map_claim_error(e),
    };

    (
        StatusCode::OK,
        Json(json!({ "employee_linked": true, "claims": claims })),
    )
        .into_response()
}

/// POST /api/v1/me/claims
pub async fn create_claim(
    State(state): State<AppState>,
    Authed(ctx): Authed,
    Json(body): Json<CreateClaimBody>,
) -> Response {
    if let Err(forbidden) = require(&ctx, "claim", "submit_self") {
        return forbidden;
    }
    let input = talentos_hr_runner::ClaimInput {
        category: body.category,
        amount: body.amount,
        expense_date: body.expense_date,
        description: body.description,
    };
    match talentos_hr_runner::create_claim(&state.pool, ctx.tenant_id, ctx.user_id.0, input).await {
        Ok(view) => (StatusCode::CREATED, Json(view)).into_response(),
        Err(e) => map_claim_error(e),
    }
}

/// POST /api/v1/me/claims/:id/cancel
pub async fn me_cancel(
    State(state): State<AppState>,
    Authed(ctx): Authed,
    Path(id): Path<Uuid>,
) -> Response {
    if let Err(forbidden) = require(&ctx, "claim", "submit_self") {
        return forbidden;
    }
    match talentos_hr_runner::cancel_claim(&state.pool, ctx.tenant_id, ctx.user_id.0, id).await {
        Ok(status) => (
            StatusCode::OK,
            Json(json!({ "claim_id": id.to_string(), "status": status })),
        )
            .into_response(),
        Err(e) => map_claim_error(e),
    }
}

/// POST /api/v1/me/claims/:id/receipt
pub async fn me_upload_receipt(
    State(state): State<AppState>,
    Authed(ctx): Authed,
    Path(id): Path<Uuid>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    if let Err(forbidden) = require(&ctx, "claim", "submit_self") {
        return forbidden;
    }
    let content_type = headers
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("application/octet-stream");

    match talentos_hr_runner::set_receipt(
        &state.pool,
        ctx.tenant_id,
        ctx.user_id.0,
        id,
        content_type,
        &body,
    )
    .await
    {
        Ok(()) => (
            StatusCode::OK,
            Json(json!({ "claim_id": id.to_string(), "has_receipt": true })),
        )
            .into_response(),
        Err(e) => map_claim_error(e),
    }
}

/// GET /api/v1/me/claims/:id/receipt
pub async fn me_download_receipt(
    State(state): State<AppState>,
    Authed(ctx): Authed,
    Path(id): Path<Uuid>,
) -> Response {
    if let Err(forbidden) = require(&ctx, "claim", "submit_self") {
        return forbidden;
    }
    download_receipt(&state, &ctx, id, true).await
}

/// GET /api/v1/claims/approvals
pub async fn approvals(State(state): State<AppState>, Authed(ctx): Authed) -> Response {
    if let Err(forbidden) = require(&ctx, "claim", "view_team") {
        return forbidden;
    }
    match talentos_hr_runner::pending_claim_approvals(&state.pool, ctx.tenant_id).await {
        Ok(claims) => (StatusCode::OK, Json(json!({ "claims": claims }))).into_response(),
        Err(e) => map_claim_error(e),
    }
}

/// POST /api/v1/claims/:id/decision
pub async fn decide(
    State(state): State<AppState>,
    Authed(ctx): Authed,
    Path(id): Path<Uuid>,
    Json(body): Json<DecideClaimBody>,
) -> Response {
    if let Err(forbidden) = require(&ctx, "claim", "approve_team") {
        return forbidden;
    }
    match talentos_hr_runner::decide_claim(
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
                "claim_id": id.to_string(),
                "status": status,
                "decided_by": ctx.user_id.to_string(),
            })),
        )
            .into_response(),
        Err(e) => map_claim_error(e),
    }
}

/// GET /api/v1/claims/:id/receipt
pub async fn hr_download_receipt(
    State(state): State<AppState>,
    Authed(ctx): Authed,
    Path(id): Path<Uuid>,
) -> Response {
    if let Err(forbidden) = require(&ctx, "claim", "view_team") {
        return forbidden;
    }
    download_receipt(&state, &ctx, id, false).await
}

async fn download_receipt(
    state: &AppState,
    ctx: &talentos_kernel::TenantContext,
    claim_id: Uuid,
    require_own: bool,
) -> Response {
    // For self-service, resolve ownership; for HR, skip the own-employee check.
    if require_own {
        let own_employee = match talentos_hr_runner::claim_employee_of_user(
            &state.pool,
            ctx.tenant_id,
            ctx.user_id.0,
        )
        .await
        {
            Ok(Some(id)) => id,
            Ok(None) => return not_found(),
            Err(e) => return internal_err(&e.to_string()),
        };
        let owner =
            match talentos_hr_runner::owner_of_claim(&state.pool, ctx.tenant_id, claim_id).await {
                Ok(Some(id)) => id,
                Ok(None) => return not_found(),
                Err(e) => return internal_err(&e.to_string()),
            };
        if owner != own_employee {
            return not_found();
        }
    }

    match talentos_hr_runner::download_receipt(&state.pool, ctx.tenant_id, claim_id).await {
        Ok(Some(file)) => (
            StatusCode::OK,
            [(
                header::CONTENT_TYPE,
                header::HeaderValue::from_str(&file.content_type).unwrap_or_else(|_| {
                    header::HeaderValue::from_static("application/octet-stream")
                }),
            )],
            file.bytes,
        )
            .into_response(),
        Ok(None) => not_found(),
        Err(e) => map_claim_error(e),
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

fn map_claim_error(e: talentos_hr_runner::ClaimError) -> Response {
    match e {
        talentos_hr_runner::ClaimError::NotFound => not_found(),
        talentos_hr_runner::ClaimError::NotPending => {
            conflict("expense claim cannot be changed in its current status")
        }
        talentos_hr_runner::ClaimError::NotCancellable => {
            conflict("expense claim cannot be cancelled in its current status")
        }
        talentos_hr_runner::ClaimError::InvalidCategory
        | talentos_hr_runner::ClaimError::InvalidAmount
        | talentos_hr_runner::ClaimError::InvalidDate
        | talentos_hr_runner::ClaimError::InvalidDescription
        | talentos_hr_runner::ClaimError::InvalidStatus => unprocessable(&e.to_string()),
        talentos_hr_runner::ClaimError::EmployeeNotFound => (
            StatusCode::FORBIDDEN,
            Json(envelope(
                "forbidden",
                "no active employee record for this account",
            )),
        )
            .into_response(),
        talentos_hr_runner::ClaimError::Receipt(receipt_err) => match receipt_err {
            talentos_hr_runner::ReceiptError::TooLarge => {
                unprocessable("receipt is too large; maximum size is 5 MB")
            }
            talentos_hr_runner::ReceiptError::InvalidType => {
                unprocessable("receipt type must be image/jpeg, image/png, or application/pdf")
            }
            talentos_hr_runner::ReceiptError::AlreadySet => {
                conflict("receipt already uploaded for this claim")
            }
            talentos_hr_runner::ReceiptError::NotFound => not_found(),
        },
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
    tracing::error!(error = %message, "claim endpoint failed");
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(envelope("internal_error", "An unexpected error occurred")),
    )
        .into_response()
}
