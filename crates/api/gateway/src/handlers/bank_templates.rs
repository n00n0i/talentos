//! Per-bank payout template registry + bank payout CSV export.

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::Deserialize;
use uuid::Uuid;

use crate::dto::responses::ErrorResponse;
use crate::middleware::auth::{require, Authed};
use crate::state::AppState;
use talentos_hr_runner::bank_templates::{
    create_bank_template, get_bank_template, list_bank_templates, BankTemplateError,
    BankTemplateInput,
};

/// GET /api/v1/payout/bank-templates — tenant layouts + global seeds.
/// Optional `?bank=Bangkok%20Bank` narrows to one bank.
pub async fn list_bank_template_handlers(
    State(state): State<AppState>,
    Authed(ctx): Authed,
    Query(query): Query<ListQuery>,
) -> Response {
    if let Err(forbidden) = require(&ctx, "payout_template", "read") {
        return forbidden;
    }
    let result = list_bank_templates(&state.pool, ctx.tenant_id, query.bank.as_deref()).await;

    match result {
        Ok(templates) => (
            StatusCode::OK,
            Json(serde_json::json!({ "bank_templates": templates })),
        )
            .into_response(),
        Err(e) => internal_err(&e.to_string()),
    }
}

#[derive(Deserialize)]
pub struct ListQuery {
    pub bank: Option<String>,
}

/// GET /api/v1/payout/bank-templates/:id — one tenant-owned layout.
pub async fn get_bank_template_handler(
    State(state): State<AppState>,
    Authed(ctx): Authed,
    Path(template_id): Path<Uuid>,
) -> Response {
    if let Err(forbidden) = require(&ctx, "payout_template", "read") {
        return forbidden;
    }
    match get_bank_template(&state.pool, ctx.tenant_id, template_id).await {
        Ok(template) => (StatusCode::OK, Json(template)).into_response(),
        Err(BankTemplateError::NotFound(_)) => not_found(),
        Err(e) => internal_err(&e.to_string()),
    }
}

/// POST /api/v1/payout/bank-templates — register a new layout (admin, audited).
/// Every new layout is unverified until an accountant signs off.
pub async fn create_bank_template_handler(
    State(state): State<AppState>,
    Authed(ctx): Authed,
    Json(input): Json<BankTemplateInput>,
) -> Response {
    if let Err(forbidden) = require(&ctx, "payout_template", "create") {
        return forbidden;
    }
    let tenant_id = ctx.tenant_id;
    let user_id = ctx.user_id.0;
    match create_bank_template(&state.pool, tenant_id, user_id, input).await {
        Ok(template) => (StatusCode::CREATED, Json(template)).into_response(),
        Err(BankTemplateError::InvalidField(msg)) => unprocessable(&format!(
            "field_order entry '{msg}' is not a known payout source"
        )),
        Err(e) => internal_err(&e.to_string()),
    }
}

/// GET /api/v1/payroll/runs/:id/bank-payout.csv — bank transfer file for an
/// approved run. Optional `?template_id=` overrides the tenant's active
/// `bank_payout` form template with a `bank_templates` row's field order.
pub async fn export_bank_payout_csv_handler(
    State(state): State<AppState>,
    Authed(ctx): Authed,
    Path(run_id): Path<Uuid>,
    Query(query): Query<TemplateQuery>,
) -> Response {
    if let Err(forbidden) = require(&ctx, "payroll_run", "export_report") {
        return forbidden;
    }
    let mut tx = match state.pool.begin_tenant(ctx.tenant_id).await {
        Ok(tx) => tx,
        Err(e) => return internal_err(&e.to_string()),
    };
    let result =
        talentos_hr_runner::export_bank_payout_csv(&mut tx, run_id, &state.pii, query.template_id)
            .await;
    let _ = tx.rollback().await;

    match result {
        Ok((filename, body)) => csv_response(filename, body),
        Err(talentos_hr_runner::FormError::NotFound) => not_found(),
        Err(e) => internal_err(&e.to_string()),
    }
}

#[derive(Deserialize)]
pub struct TemplateQuery {
    pub template_id: Option<Uuid>,
}

fn csv_response(filename: String, body: String) -> Response {
    (
        StatusCode::OK,
        [
            (
                axum::http::header::CONTENT_TYPE,
                "text/csv; charset=utf-8".to_string(),
            ),
            (
                axum::http::header::CONTENT_DISPOSITION,
                format!("attachment; filename=\"{filename}\""),
            ),
        ],
        body,
    )
        .into_response()
}

fn unprocessable(message: &str) -> Response {
    error_response(StatusCode::UNPROCESSABLE_ENTITY, "invalid_request", message)
}

fn not_found() -> Response {
    error_response(StatusCode::NOT_FOUND, "not_found", "Not found")
}

fn internal_err(message: &str) -> Response {
    tracing::error!(error = %message, "bank template endpoint failed");
    error_response(
        StatusCode::INTERNAL_SERVER_ERROR,
        "internal_error",
        "An unexpected error occurred",
    )
}

fn error_response(status: StatusCode, error: &str, message: &str) -> Response {
    (
        status,
        Json(ErrorResponse {
            error: error.into(),
            message: message.into(),
            details: vec![],
            request_id: Uuid::new_v4().to_string(),
            retry_after_seconds: None,
        }),
    )
        .into_response()
}
