//! Employee self-service: a linked user can read and download only their own
//! payslips and clock in/out. Tenant isolation is RLS; the employee link is the
//! ownership check.

use axum::{
    extract::{connect_info::ConnectInfo, Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use uuid::Uuid;

use crate::dto::responses::ErrorResponse;
use crate::handlers::attendance::map_attendance_error;
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
        r#"SELECT p.id, r.period_start, r.period_end, p.gross_earnings, p.net_pay, p.status,
                  pa.acknowledged_at
             FROM payslips p
             JOIN payroll_runs r ON r.id = p.payroll_run_id
             JOIN employees e ON e.id = p.employee_id
             LEFT JOIN payslip_acknowledgements pa
                    ON pa.payslip_id = p.id AND pa.tenant_id = p.tenant_id
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
                        "acknowledged_at": row.get::<Option<chrono::DateTime<chrono::Utc>>, _>("acknowledged_at").map(|t| t.to_rfc3339()),
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

/// GET /api/v1/me/payslips/:id/50tawi — own withholding certificate.
pub async fn payslip_50tawi(
    State(state): State<AppState>,
    Authed(ctx): Authed,
    Path(id): Path<Uuid>,
) -> Response {
    if let Err(forbidden) = require(&ctx, "payslip", "view_self") {
        return forbidden;
    }
    let mut tx = match state.pool.begin_tenant(ctx.tenant_id).await {
        Ok(tx) => tx,
        Err(e) => return internal_err(&e.to_string()),
    };
    let owned: Option<Uuid> = match sqlx::query_scalar(
        r#"SELECT p.id FROM payslips p
             JOIN employees e ON e.id = p.employee_id
            WHERE p.id = $1 AND p.tenant_id = $2 AND e.user_id = $3"#,
    )
    .bind(id)
    .bind(ctx.tenant_id.0)
    .bind(ctx.user_id.0)
    .fetch_optional(&mut *tx)
    .await
    {
        Ok(v) => v,
        Err(e) => {
            let _ = tx.rollback().await;
            return internal_err(&e.to_string());
        }
    };
    if owned.is_none() {
        let _ = tx.rollback().await;
        return not_found();
    }
    let rendered = talentos_hr_runner::render_50tawi(&mut tx, id, &state.pii).await;
    let _ = tx.rollback().await;
    match rendered {
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

/// POST /api/v1/me/payslips/:id/acknowledge — idempotent receipt confirmation.
pub async fn acknowledge(
    State(state): State<AppState>,
    ConnectInfo(addr): ConnectInfo<std::net::SocketAddr>,
    Authed(ctx): Authed,
    Path(id): Path<Uuid>,
) -> Response {
    if let Err(forbidden) = require(&ctx, "payslip", "view_self") {
        return forbidden;
    }
    let mut tx = match state.pool.begin_tenant(ctx.tenant_id).await {
        Ok(tx) => tx,
        Err(e) => return internal_err(&e.to_string()),
    };

    let owned: Option<Uuid> = match sqlx::query_scalar(
        r#"SELECT p.id FROM payslips p
             JOIN employees e ON e.id = p.employee_id
            WHERE p.id = $1 AND p.tenant_id = $2 AND e.user_id = $3"#,
    )
    .bind(id)
    .bind(ctx.tenant_id.0)
    .bind(ctx.user_id.0)
    .fetch_optional(&mut *tx)
    .await
    {
        Ok(found) => found,
        Err(e) => {
            let _ = tx.rollback().await;
            return internal_err(&e.to_string());
        }
    };

    if owned.is_none() {
        let _ = tx.rollback().await;
        return not_found();
    }

    if let Err(e) = sqlx::query(
        "INSERT INTO payslip_acknowledgements (payslip_id, tenant_id, user_id, ip_address)
         VALUES ($1, $2, $3, $4) ON CONFLICT (payslip_id) DO NOTHING",
    )
    .bind(id)
    .bind(ctx.tenant_id.0)
    .bind(ctx.user_id.0)
    .bind(sqlx::types::ipnetwork::IpNetwork::from(addr.ip()))
    .execute(&mut *tx)
    .await
    {
        let _ = tx.rollback().await;
        return internal_err(&e.to_string());
    }

    let acknowledged_at: Option<chrono::DateTime<chrono::Utc>> = match sqlx::query_scalar(
        "SELECT acknowledged_at FROM payslip_acknowledgements WHERE payslip_id = $1 AND tenant_id = $2",
    )
    .bind(id)
    .bind(ctx.tenant_id.0)
    .fetch_optional(&mut *tx)
    .await
    {
        Ok(ts) => ts,
        Err(e) => {
            let _ = tx.rollback().await;
            return internal_err(&e.to_string());
        }
    };

    let _ = tx.commit().await;
    match acknowledged_at {
        Some(ts) => (
            StatusCode::OK,
            Json(json!({ "acknowledged_at": ts.to_rfc3339() })),
        )
            .into_response(),
        None => internal_err("acknowledgement not found after insert"),
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

/// Body for a mobile punch.
#[derive(Debug, Deserialize)]
pub struct PunchBody {
    pub kind: String,
    pub lat: f64,
    pub lng: f64,
    pub accuracy_m: Option<f64>,
    pub client_id: Option<String>,
    /// RFC 3339 instant for a punch queued while offline.
    pub client_time: Option<String>,
}

/// Response for a mobile punch.
#[derive(Debug, Serialize)]
pub struct PunchResponse {
    pub work_date: String,
    pub worked_minutes: i32,
    pub late_minutes: i32,
    pub early_leave_minutes: i32,
    pub ot_minutes: i32,
    pub ot_kind: String,
    pub incomplete: bool,
}

/// POST /api/v1/me/attendance/punch
pub async fn punch(
    State(state): State<AppState>,
    Authed(ctx): Authed,
    Json(body): Json<PunchBody>,
) -> Response {
    if let Err(forbidden) = require(&ctx, "attendance", "punch_self") {
        return forbidden;
    }

    let kind = match body.kind.as_str() {
        "in" => talentos_attendance::PunchKind::In,
        "out" => talentos_attendance::PunchKind::Out,
        _ => {
            return (
                StatusCode::BAD_REQUEST,
                Json(ErrorResponse {
                    error: "bad_request".into(),
                    message: "kind must be 'in' or 'out'".into(),
                    details: vec![],
                    request_id: Uuid::new_v4().to_string(),
                    retry_after_seconds: None,
                }),
            )
                .into_response()
        }
    };

    let input = talentos_hr_runner::attendance::PunchInput {
        kind,
        lat: body.lat,
        lng: body.lng,
        accuracy_m: body.accuracy_m,
        client_id: body.client_id,
        client_time: match body.client_time {
            Some(value) => match chrono::DateTime::parse_from_rfc3339(&value) {
                Ok(t) => Some(t.with_timezone(&chrono::Utc)),
                Err(_) => {
                    return error_response(
                        StatusCode::UNPROCESSABLE_ENTITY,
                        "invalid_client_time",
                        "client_time must be an RFC 3339 timestamp",
                    )
                }
            },
            None => None,
        },
    };

    match talentos_hr_runner::attendance::punch(&state.pool, ctx.tenant_id, ctx.user_id.0, input)
        .await
    {
        Ok(day) => (
            StatusCode::OK,
            Json(PunchResponse {
                work_date: day.work_date.to_string(),
                worked_minutes: day.worked_minutes,
                late_minutes: day.late_minutes,
                early_leave_minutes: day.early_leave_minutes,
                ot_minutes: day.ot_minutes,
                ot_kind: day.ot_kind,
                incomplete: day.incomplete,
            }),
        )
            .into_response(),
        Err(e) => map_attendance_error(e),
    }
}

#[derive(Debug, Deserialize)]
pub struct MonthQuery {
    pub month: Option<String>,
}

/// GET /api/v1/me/attendance?month=YYYY-MM (defaults to the current Bangkok month)
pub async fn my_attendance(
    State(state): State<AppState>,
    Authed(ctx): Authed,
    Query(query): Query<MonthQuery>,
) -> Response {
    use chrono::{Datelike, NaiveDate};
    if let Err(forbidden) = require(&ctx, "attendance", "punch_self") {
        return forbidden;
    }
    let first = match query.month.as_deref() {
        Some(m) => NaiveDate::parse_from_str(&format!("{m}-01"), "%Y-%m-%d").ok(),
        None => {
            let today = chrono::Utc::now()
                .with_timezone(&talentos_attendance::bangkok_offset())
                .date_naive();
            NaiveDate::from_ymd_opt(today.year(), today.month(), 1)
        }
    };
    let Some(first) = first else {
        return error_response(
            StatusCode::UNPROCESSABLE_ENTITY,
            "invalid_month",
            "month must be YYYY-MM",
        );
    };
    let last = (first + chrono::Months::new(1)).pred_opt().unwrap_or(first);

    match talentos_hr_runner::attendance::list_my_days(
        &state.pool,
        ctx.tenant_id,
        ctx.user_id.0,
        first,
        last,
    )
    .await
    {
        Ok(days) => Json(json!({ "employee_linked": true, "days": days })).into_response(),
        Err(talentos_hr_runner::attendance::AttendanceError::EmployeeNotLinked) => {
            Json(json!({ "employee_linked": false, "days": [] })).into_response()
        }
        Err(e) => map_attendance_error(e),
    }
}
