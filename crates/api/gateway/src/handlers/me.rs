//! Employee self-service: a linked user can read and download only their own
//! payslips. Tenant isolation is RLS; the employee link is the ownership check.

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

/// GET /api/v1/me/payslips
pub async fn payslips(State(state): State<AppState>, Authed(ctx): Authed) -> Response {
    if let Err(forbidden) = require(&ctx, "payslip", "view_self") {
        return forbidden;
    }
    let mut tx = match state.pool.begin_tenant(ctx.tenant_id).await {
        Ok(tx) => tx,
        Err(e) => return internal_err(&e.to_string()),
    };
    let employee: Option<Uuid> = sqlx::query_scalar(
        "SELECT id FROM employees WHERE tenant_id = $1 AND user_id = $2 AND status <> 'terminated'",
    )
    .bind(ctx.tenant_id.0)
    .bind(ctx.user_id.0)
    .fetch_optional(&mut *tx)
    .await
    .unwrap_or(None);

    if employee.is_none() {
        let _ = tx.rollback().await;
        return (
            StatusCode::OK,
            Json(json!({ "employee_linked": false, "payslips": [] })),
        )
            .into_response();
    }

    let rows = sqlx::query(
        r#"SELECT p.id, r.period_start, r.period_end, p.gross_earnings, p.net_pay, p.status
             FROM payslips p
             JOIN payroll_runs r ON r.id = p.payroll_run_id
             JOIN employees e ON e.id = p.employee_id
            WHERE p.tenant_id = $1 AND e.user_id = $2
            ORDER BY r.period_start DESC
            LIMIT 50"#,
    )
    .bind(ctx.tenant_id.0)
    .bind(ctx.user_id.0)
    .fetch_all(&mut *tx)
    .await;
    let _ = tx.rollback().await;

    match rows {
        Ok(rows) => {
            use sqlx::Row;
            let payslips: Vec<serde_json::Value> = rows
                .iter()
                .map(|row| {
                    json!({
                        "id": row.get::<Uuid, _>("id"),
                        "period_start": row.get::<chrono::NaiveDate, _>("period_start").to_string(),
                        "period_end": row.get::<chrono::NaiveDate, _>("period_end").to_string(),
                        "gross": row.get::<rust_decimal::Decimal, _>("gross_earnings").to_string(),
                        "net": row.get::<rust_decimal::Decimal, _>("net_pay").to_string(),
                        "status": row.get::<String, _>("status"),
                    })
                })
                .collect();
            (
                StatusCode::OK,
                Json(json!({ "employee_linked": true, "payslips": payslips })),
            )
                .into_response()
        }
        Err(e) => internal_err(&e.to_string()),
    }
}

/// GET /api/v1/me/payslips/:id/render
pub async fn render(
    State(state): State<AppState>,
    Authed(ctx): Authed,
    Path(id): Path<Uuid>,
) -> Response {
    if let Err(forbidden) = require(&ctx, "payslip", "view_self") {
        return forbidden;
    }
    match owned(&state, &ctx, id).await {
        Ok(true) => {}
        Ok(false) => return not_found(),
        Err(e) => return internal_err(&e),
    }
    match talentos_payroll_runner::render_payslip(&state.pool, ctx.tenant_id, id).await {
        Ok(html) => (
            StatusCode::OK,
            [(axum::http::header::CONTENT_TYPE, "text/html; charset=utf-8")],
            html,
        )
            .into_response(),
        Err(talentos_payroll_runner::RunError::NotFound(_)) => not_found(),
        Err(e) => {
            tracing::error!(error = %e, "self payslip render failed");
            internal_err("payslip could not be rendered")
        }
    }
}

/// GET /api/v1/me/payslips/:id/pdf
pub async fn pdf(
    State(state): State<AppState>,
    Authed(ctx): Authed,
    Path(id): Path<Uuid>,
) -> Response {
    if let Err(forbidden) = require(&ctx, "payslip", "view_self") {
        return forbidden;
    }
    match owned(&state, &ctx, id).await {
        Ok(true) => {}
        Ok(false) => return not_found(),
        Err(e) => return internal_err(&e),
    }
    let html = match talentos_payroll_runner::render_payslip(&state.pool, ctx.tenant_id, id).await {
        Ok(html) => html,
        Err(talentos_payroll_runner::RunError::NotFound(_)) => return not_found(),
        Err(e) => {
            tracing::error!(error = %e, "self payslip render failed");
            return internal_err("payslip could not be rendered");
        }
    };
    let cfg = talentos_payroll_runner::pdf::PdfConfig::default();
    match talentos_payroll_runner::pdf::get_or_render_pdf(&cfg, ctx.tenant_id.0, id, &html).await {
        Ok(bytes) => (
            StatusCode::OK,
            [
                (axum::http::header::CONTENT_TYPE, "application/pdf"),
                (
                    axum::http::header::CONTENT_DISPOSITION,
                    "inline; filename=\"payslip.pdf\"",
                ),
            ],
            bytes,
        )
            .into_response(),
        Err(talentos_payroll_runner::RunError::PdfUnavailable(msg)) => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(ErrorResponse {
                error: "pdf_unavailable".into(),
                message: msg,
                details: vec![],
                request_id: Uuid::new_v4().to_string(),
                retry_after_seconds: None,
            }),
        )
            .into_response(),
        Err(e) => {
            tracing::error!(error = %e, "self payslip pdf failed");
            internal_err("payslip pdf could not be generated")
        }
    }
}

async fn owned(
    state: &AppState,
    ctx: &talentos_kernel::TenantContext,
    payslip_id: Uuid,
) -> Result<bool, String> {
    let mut tx = state
        .pool
        .begin_tenant(ctx.tenant_id)
        .await
        .map_err(|e| e.to_string())?;
    let found: bool = sqlx::query_scalar(
        r#"SELECT EXISTS (
             SELECT 1 FROM payslips p
             JOIN employees e ON e.id = p.employee_id
             WHERE p.id = $1 AND p.tenant_id = $2 AND e.user_id = $3
           )"#,
    )
    .bind(payslip_id)
    .bind(ctx.tenant_id.0)
    .bind(ctx.user_id.0)
    .fetch_one(&mut *tx)
    .await
    .map_err(|e| e.to_string())?;
    let _ = tx.rollback().await;
    Ok(found)
}

fn not_found() -> Response {
    error_response(StatusCode::NOT_FOUND, "not_found", "payslip not found")
}

fn internal_err(message: &str) -> Response {
    tracing::error!(error = %message, "self-service endpoint failed");
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
