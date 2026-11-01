//! PDPA admin endpoints: retention policy view, employee PII purge (DSAR),
//! and the batch retention-purge job. Gated on the `privacy.purge` RBAC
//! permission (hr_admin only); every purge is audited by the service.

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use uuid::Uuid;

use crate::dto::responses::ErrorResponse;
use crate::middleware::auth::{require, Authed};
use crate::state::AppState;
use talentos_hr_runner::pdpa::{self, PdpaError};

/// GET /api/v1/privacy/policy — the tenant's retention policy.
pub async fn policy(State(state): State<AppState>, Authed(ctx): Authed) -> Response {
    if let Err(forbidden) = require(&ctx, "privacy", "purge") {
        return forbidden;
    }
    match pdpa::get_policy(&state.pool, ctx.tenant_id).await {
        Ok(p) => Json(serde_json::json!({
            "payroll_retention_years": p.payroll_retention_years,
            "ops_retention_months": p.ops_retention_months,
        }))
        .into_response(),
        Err(e) => map_error(e),
    }
}

/// POST /api/v1/privacy/purge/:employee_id — DSAR anonymisation for one
/// employee.
pub async fn purge(
    State(state): State<AppState>,
    Authed(ctx): Authed,
    Path(employee_id): Path<Uuid>,
) -> Response {
    if let Err(forbidden) = require(&ctx, "privacy", "purge") {
        return forbidden;
    }
    match pdpa::purge_employee_pii(
        &state.pool,
        ctx.tenant_id,
        employee_id,
        ctx.user_id.0,
        "user",
    )
    .await
    {
        Ok(r) => Json(serde_json::json!({
            "employee_id": r.employee_id,
            "pii_anonymised": r.pii_anonymised,
            "candidate_rows_anonymised": r.candidate_rows_anonymised,
            "operational_rows_removed": r.operational_rows_removed,
        }))
        .into_response(),
        Err(e) => map_error(e),
    }
}

/// POST /api/v1/privacy/retention-purge — batch job: purge every terminated
/// employee whose operational-data retention window has elapsed.
pub async fn retention_purge(State(state): State<AppState>, Authed(ctx): Authed) -> Response {
    if let Err(forbidden) = require(&ctx, "privacy", "purge") {
        return forbidden;
    }
    match pdpa::run_retention_purge(&state.pool, ctx.tenant_id, ctx.user_id.0, "user").await {
        Ok(results) => Json(serde_json::json!({
            "purged": results.len(),
            "results": results,
        }))
        .into_response(),
        Err(e) => map_error(e),
    }
}

fn map_error(e: PdpaError) -> Response {
    match e {
        PdpaError::EmployeeNotFound => (
            StatusCode::NOT_FOUND,
            Json(ErrorResponse {
                error: "employee_not_found".into(),
                message: e.to_string(),
                details: vec![],
                request_id: Uuid::new_v4().to_string(),
                retry_after_seconds: None,
            }),
        )
            .into_response(),
        PdpaError::Db(_) => {
            tracing::error!(error = %e, "pdpa endpoint failed");
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
    }
}
