//! Statutory form templates (50 ทวิ / สปส.1-10) and their render endpoints.

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

/// GET /api/v1/forms/:type — active template (tenant override else global).
pub async fn get_form(
    State(state): State<AppState>,
    Authed(ctx): Authed,
    Path(form_type): Path<String>,
) -> Response {
    if let Err(forbidden) = require(&ctx, "settings", "view_company") {
        return forbidden;
    }
    let mut tx = match state.pool.begin_tenant(ctx.tenant_id).await {
        Ok(tx) => tx,
        Err(e) => return internal_err(&e.to_string()),
    };
    let result = talentos_hr_runner::load_active_template(&mut tx, &form_type).await;
    let _ = tx.rollback().await;

    match result {
        Ok(template) => (
            StatusCode::OK,
            Json(serde_json::json!({
                "form_type": template.form_type,
                "version": template.version,
                "config": template.config,
                "is_tenant_override": template.is_tenant_override,
            })),
        )
            .into_response(),
        Err(talentos_hr_runner::FormError::NotFound) => not_found(),
        Err(talentos_hr_runner::FormError::UnknownFormType) => bad_request("unknown form type"),
        Err(e) => internal_err(&e.to_string()),
    }
}

/// PUT /api/v1/forms/:type — upsert the tenant template (new version).
pub async fn put_form(
    State(state): State<AppState>,
    Authed(ctx): Authed,
    Path(form_type): Path<String>,
    Json(config): Json<talentos_hr_runner::FormConfig>,
) -> Response {
    if let Err(forbidden) = require(&ctx, "settings", "manage_company") {
        return forbidden;
    }
    let mut tx = match state.pool.begin_tenant(ctx.tenant_id).await {
        Ok(tx) => tx,
        Err(e) => return internal_err(&e.to_string()),
    };
    let result =
        talentos_hr_runner::save_template(&mut tx, &form_type, &config, ctx.user_id.0).await;
    if let Err(e) = tx.commit().await {
        return internal_err(&e.to_string());
    }

    match result {
        Ok(version) => (
            StatusCode::OK,
            Json(serde_json::json!({ "version": version })),
        )
            .into_response(),
        Err(talentos_hr_runner::FormError::Invalid(msg)) => (
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(ErrorResponse {
                error: "invalid_template".into(),
                message: msg,
                details: vec![],
                request_id: Uuid::new_v4().to_string(),
                retry_after_seconds: None,
            }),
        )
            .into_response(),
        Err(talentos_hr_runner::FormError::UnknownFormType) => bad_request("unknown form type"),
        Err(e) => internal_err(&e.to_string()),
    }
}

/// GET /api/v1/payroll/payslips/:id/50tawi — printable 50 ทวิ HTML.
pub async fn render_50tawi(
    State(state): State<AppState>,
    Authed(ctx): Authed,
    Path(payslip_id): Path<Uuid>,
) -> Response {
    if let Err(forbidden) = require(&ctx, "payslip", "view_all") {
        return forbidden;
    }
    let mut tx = match state.pool.begin_tenant(ctx.tenant_id).await {
        Ok(tx) => tx,
        Err(e) => return internal_err(&e.to_string()),
    };
    let result = talentos_hr_runner::render_50tawi(&mut tx, payslip_id, &state.pii).await;
    let _ = tx.rollback().await;

    match result {
        Ok(html) => (
            StatusCode::OK,
            [(axum::http::header::CONTENT_TYPE, "text/html; charset=utf-8")],
            html,
        )
            .into_response(),
        Err(talentos_hr_runner::FormError::NotFound) => not_found(),
        Err(e) => internal_err(&e.to_string()),
    }
}

/// GET /api/v1/payroll/runs/:id/sso1-10.csv — สปส.1-10 CSV (approved runs).
pub async fn export_sso1_10_csv(
    State(state): State<AppState>,
    Authed(ctx): Authed,
    Path(run_id): Path<Uuid>,
) -> Response {
    if let Err(forbidden) = require(&ctx, "payroll_run", "export_report") {
        return forbidden;
    }
    let mut tx = match state.pool.begin_tenant(ctx.tenant_id).await {
        Ok(tx) => tx,
        Err(e) => return internal_err(&e.to_string()),
    };
    let result = talentos_hr_runner::export_sso1_10_csv(&mut tx, run_id, &state.pii).await;
    let _ = tx.rollback().await;

    match result {
        Ok((filename, body)) => csv_response(filename, body),
        Err(talentos_hr_runner::FormError::NotFound) => not_found(),
        Err(e) => internal_err(&e.to_string()),
    }
}

/// GET /api/v1/payroll/runs/:id/pnd1.csv — ภ.ง.ด.1 CSV (approved runs).
pub async fn export_pnd1_csv(
    State(state): State<AppState>,
    Authed(ctx): Authed,
    Path(run_id): Path<Uuid>,
) -> Response {
    if let Err(forbidden) = require(&ctx, "payroll_run", "export_report") {
        return forbidden;
    }
    let mut tx = match state.pool.begin_tenant(ctx.tenant_id).await {
        Ok(tx) => tx,
        Err(e) => return internal_err(&e.to_string()),
    };
    let result = talentos_hr_runner::export_pnd1_csv(&mut tx, run_id, &state.pii).await;
    let _ = tx.rollback().await;

    match result {
        Ok((filename, body)) => csv_response(filename, body),
        Err(talentos_hr_runner::FormError::NotFound) => not_found(),
        Err(e) => internal_err(&e.to_string()),
    }
}

/// GET /api/v1/reports/pnd1a.csv?year= — ภ.ง.ด.1ก CSV (annual withholding).
pub async fn export_pnd1a_csv(
    State(state): State<AppState>,
    Authed(ctx): Authed,
    Query(query): Query<Pnd1aQuery>,
) -> Response {
    if let Err(forbidden) = require(&ctx, "payroll_run", "export_report") {
        return forbidden;
    }
    let year = match query.year {
        Some(y) if (1950..=2100).contains(&y) => y,
        _ => return unprocessable("year is required (a four-digit year)"),
    };
    let mut tx = match state.pool.begin_tenant(ctx.tenant_id).await {
        Ok(tx) => tx,
        Err(e) => return internal_err(&e.to_string()),
    };
    let result = talentos_hr_runner::export_pnd1a_csv(&mut tx, year, &state.pii).await;
    let _ = tx.rollback().await;

    match result {
        Ok((filename, body)) => csv_response(filename, body),
        Err(e) => internal_err(&e.to_string()),
    }
}

#[derive(Deserialize)]
pub struct Pnd1aQuery {
    pub year: Option<i32>,
}

fn unprocessable(message: &str) -> Response {
    error_response(StatusCode::UNPROCESSABLE_ENTITY, "invalid_request", message)
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

fn bad_request(message: &str) -> Response {
    error_response(StatusCode::BAD_REQUEST, "invalid_request", message)
}

fn not_found() -> Response {
    error_response(StatusCode::NOT_FOUND, "not_found", "Not found")
}

fn internal_err(message: &str) -> Response {
    tracing::error!(error = %message, "forms endpoint failed");
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
