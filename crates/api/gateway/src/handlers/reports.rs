use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use chrono::NaiveDate;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::dto::responses::ErrorResponse;
use crate::middleware::auth::{require, Authed};
use crate::state::AppState;

/// GET /api/v1/reports/headcount
pub async fn headcount(State(state): State<AppState>, Authed(ctx): Authed) -> Response {
    if let Err(forbidden) = require(&ctx, "payroll_run", "export_report") {
        return forbidden;
    }
    let mut tx = match state.pool.begin_tenant(ctx.tenant_id).await {
        Ok(tx) => tx,
        Err(e) => return internal_err(e),
    };
    let report = match talentos_hr_runner::reports::headcount(&mut tx).await {
        Ok(r) => r,
        Err(e) => {
            let _ = tx.rollback().await;
            return internal_err_msg(&format!("report query failed: {e}"));
        }
    };
    let _ = tx.rollback().await;

    Json(HeadcountResponse {
        active: report.active,
        probation: report.probation,
        leave: report.leave,
        terminated_last_12m: report.terminated_last_12m,
        by_department: report
            .by_department
            .into_iter()
            .map(|d| DepartmentCountItem {
                department: d.department,
                count: d.count,
            })
            .collect(),
    })
    .into_response()
}

/// GET /api/v1/reports/payroll-cost?from=&to=
pub async fn payroll_cost(
    State(state): State<AppState>,
    Authed(ctx): Authed,
    Query(query): Query<ReportRangeQuery>,
) -> Response {
    if let Err(forbidden) = require(&ctx, "payroll_run", "export_report") {
        return forbidden;
    }
    let (from, to) = match parse_range(&query) {
        Ok(dates) => dates,
        Err(response) => return response,
    };
    let mut tx = match state.pool.begin_tenant(ctx.tenant_id).await {
        Ok(tx) => tx,
        Err(e) => return internal_err(e),
    };
    let report = match talentos_hr_runner::reports::payroll_cost(&mut tx, from, to).await {
        Ok(r) => r,
        Err(e) => {
            let _ = tx.rollback().await;
            return internal_err_msg(&format!("report query failed: {e}"));
        }
    };
    let _ = tx.rollback().await;

    Json(PayrollCostResponse {
        rows: report
            .rows
            .into_iter()
            .map(|r| PayrollCostRowItem {
                department: r.department,
                employees: r.employees,
                gross: r.gross.to_string(),
                net: r.net.to_string(),
                tax: r.tax.to_string(),
                sso: r.sso.to_string(),
                pf: r.pf.to_string(),
            })
            .collect(),
    })
    .into_response()
}

/// GET /api/v1/reports/ot-trend?from=&to=
pub async fn ot_trend(
    State(state): State<AppState>,
    Authed(ctx): Authed,
    Query(query): Query<ReportRangeQuery>,
) -> Response {
    if let Err(forbidden) = require(&ctx, "payroll_run", "export_report") {
        return forbidden;
    }
    let (from, to) = match parse_range(&query) {
        Ok(dates) => dates,
        Err(response) => return response,
    };
    let mut tx = match state.pool.begin_tenant(ctx.tenant_id).await {
        Ok(tx) => tx,
        Err(e) => return internal_err(e),
    };
    let report = match talentos_hr_runner::reports::ot_trend(&mut tx, from, to).await {
        Ok(r) => r,
        Err(e) => {
            let _ = tx.rollback().await;
            return internal_err_msg(&format!("report query failed: {e}"));
        }
    };
    let _ = tx.rollback().await;

    Json(OtTrendResponse {
        rows: report
            .rows
            .into_iter()
            .map(|r| OtTrendRowItem {
                month: r.month,
                ot_minutes: r.ot_minutes,
                approved_days: r.approved_days,
            })
            .collect(),
    })
    .into_response()
}

/// GET /api/v1/payroll/runs/:id/export.gl.csv
pub async fn export_gl_csv(
    State(state): State<AppState>,
    Authed(ctx): Authed,
    Path(run_id): Path<Uuid>,
) -> Response {
    if let Err(forbidden) = require(&ctx, "payroll_run", "export_report") {
        return forbidden;
    }
    let mut tx = match state.pool.begin_tenant(ctx.tenant_id).await {
        Ok(tx) => tx,
        Err(e) => return internal_err(e),
    };
    let (filename, body) = match talentos_hr_runner::reports::export_gl_csv(&mut tx, run_id).await {
        Ok(pair) => pair,
        Err(talentos_hr_runner::reports::ReportError::NotFound) => {
            let _ = tx.rollback().await;
            return not_found();
        }
        Err(e) => {
            let _ = tx.rollback().await;
            return internal_err_msg(&format!("gl export failed: {e}"));
        }
    };
    let _ = tx.rollback().await;

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

#[allow(clippy::result_large_err)]
fn parse_range(query: &ReportRangeQuery) -> Result<(NaiveDate, NaiveDate), Response> {
    let from = match query.from {
        Some(d) => d,
        None => return Err(bad_request("from is required")),
    };
    let to = match query.to {
        Some(d) => d,
        None => return Err(bad_request("to is required")),
    };
    if to < from {
        return Err(bad_request("to must be on or after from"));
    }
    Ok((from, to))
}

fn bad_request(message: &str) -> Response {
    (
        StatusCode::BAD_REQUEST,
        Json(ErrorResponse {
            error: "invalid_request".into(),
            message: message.to_string(),
            details: vec![],
            request_id: Uuid::new_v4().to_string(),
            retry_after_seconds: None,
        }),
    )
        .into_response()
}

fn not_found() -> Response {
    (
        StatusCode::NOT_FOUND,
        Json(ErrorResponse {
            error: "not_found".into(),
            message: "Payroll run not found".into(),
            details: vec![],
            request_id: Uuid::new_v4().to_string(),
            retry_after_seconds: None,
        }),
    )
        .into_response()
}

fn internal_err(e: sqlx::Error) -> Response {
    tracing::error!(error = %e, "database error");
    internal_err_msg("An unexpected error occurred")
}

fn internal_err_msg(message: &str) -> Response {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(ErrorResponse {
            error: "internal_error".into(),
            message: message.to_string(),
            details: vec![],
            request_id: Uuid::new_v4().to_string(),
            retry_after_seconds: None,
        }),
    )
        .into_response()
}

#[derive(Deserialize)]
pub struct ReportRangeQuery {
    pub from: Option<NaiveDate>,
    pub to: Option<NaiveDate>,
}

#[derive(Serialize)]
pub struct HeadcountResponse {
    pub active: i64,
    pub probation: i64,
    pub leave: i64,
    pub terminated_last_12m: i64,
    pub by_department: Vec<DepartmentCountItem>,
}

#[derive(Serialize)]
pub struct DepartmentCountItem {
    pub department: String,
    pub count: i64,
}

#[derive(Serialize)]
pub struct PayrollCostResponse {
    pub rows: Vec<PayrollCostRowItem>,
}

#[derive(Serialize)]
pub struct PayrollCostRowItem {
    pub department: String,
    pub employees: i64,
    pub gross: String,
    pub net: String,
    pub tax: String,
    pub sso: String,
    pub pf: String,
}

#[derive(Serialize)]
pub struct OtTrendResponse {
    pub rows: Vec<OtTrendRowItem>,
}

#[derive(Serialize)]
pub struct OtTrendRowItem {
    pub month: String,
    pub ot_minutes: i64,
    pub approved_days: i64,
}
