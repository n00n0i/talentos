use std::sync::Arc;
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use axum::extract::connect_info::ConnectInfo;
use std::net::SocketAddr;
use sqlx::PgPool;
use uuid::Uuid;
use chrono::NaiveDate;
use serde::{Deserialize, Serialize};
use talents_kernel::{TenantId, EntityId, UserId, Money};
use talents_db::TenantPool;

use crate::dto::responses::{ErrorResponse, HealthResponse};
use crate::middleware::auth::Authed;
use crate::ratelimit;
use crate::state::AppState;

/// POST /api/v1/payroll/runs
pub async fn start_payroll_run(
    State(state): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    Authed(ctx): Authed,
    Json(body): Json<StartPayrollRunRequest>,
) -> Response {
    // Validate period
    if body.period_end <= body.period_start {
        return (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: "invalid_request".into(),
                message: "period_end must be after period_start".into(),
                details: vec![],
                request_id: Uuid::new_v4().to_string(),
                retry_after_seconds: None,
            }),
        ).into_response();
    }

    // Check for existing run for this period (spec §7: concurrent run prevention)
    let existing = sqlx::query!(
        "SELECT id FROM payroll_runs WHERE tenant_id = $1 AND entity_id = $2 AND period_start = $3 AND period_end = $4",
        ctx.tenant_id.0,
        ctx.entity_id.map(|e| e.0),
        body.period_start,
        body.period_end
    )
    .fetch_optional(state.pool.pool())
    .await;

    if let Ok(Some(_)) = existing {
        return (
            StatusCode::CONFLICT,
            Json(ErrorResponse {
                error: "concurrent_run".into(),
                message: "Payroll run already exists for this period".into(),
                details: vec![],
                request_id: Uuid::new_v4().to_string(),
                retry_after_seconds: None,
            }),
        ).into_response();
    }

    // Create payroll run in draft status
    let run_id = Uuid::new_v4();
    let now = chrono::Utc::now();

    let _ = sqlx::query!(
        r#"INSERT INTO payroll_runs (id, tenant_id, entity_id, period_start, period_end, payment_date, status, total_employees, created_by, created_at)
           VALUES ($1, $2, $3, $4, $5, $6, 'draft', 0, $7, $8)"#,
        run_id,
        ctx.tenant_id.0,
        ctx.entity_id.map(|e| e.0),
        body.period_start,
        body.period_end,
        body.payment_date,
        ctx.user_id.0,
        now
    )
    .execute(state.pool.pool())
    .await;

    // TODO: Start Temporal workflow here — for Phase 2, we'll create the run and return the run_id
    // The worker will pick it up via Temporal

    (
        StatusCode::CREATED,
        Json(StartPayrollRunResponse {
            run_id: run_id.to_string(),
            status: "draft".into(),
            period_start: body.period_start.to_string(),
            period_end: body.period_end.to_string(),
            payment_date: body.payment_date.to_string(),
        }),
    ).into_response()
}

/// GET /api/v1/payroll/runs/:id
pub async fn get_payroll_run(
    State(state): State<AppState>,
    Authed(ctx): Authed,
    Path(id): Path<Uuid>,
) -> Response {
    let row = sqlx::query!(
        r#"SELECT id, tenant_id, entity_id, period_start, period_end, payment_date, status, total_employees,
                  total_gross, total_net, total_tax, total_sso, total_pf, workflow_id, created_by, created_at, updated_at
           FROM payroll_runs WHERE id = $1 AND tenant_id = $2"#,
        id,
        ctx.tenant_id.0
    )
    .fetch_optional(state.pool.pool())
    .await;

    let run = match row {
        Ok(Some(r)) => r,
        Ok(None) => return not_found(),
        Err(e) => return internal_err(e),
    };

    (
        StatusCode::OK,
        Json(GetPayrollRunResponse {
            run_id: run.id.to_string(),
            tenant_id: run.tenant_id.to_string(),
            entity_id: run.entity_id.map(|e| e.to_string()),
            period_start: run.period_start.to_string(),
            period_end: run.period_end.to_string(),
            payment_date: run.payment_date.to_string(),
            status: run.status,
            total_employees: run.total_employees,
            total_gross: run.total_gross.map(|v| v.to_string()),
            total_net: run.total_net.map(|v| v.to_string()),
            total_tax: run.total_tax.map(|v| v.to_string()),
            total_sso: run.total_sso.map(|v| v.to_string()),
            total_pf: run.total_pf.map(|v| v.to_string()),
            workflow_id: run.workflow_id,
            created_by: run.created_by.to_string(),
            created_at: run.created_at.to_rfc3339(),
            updated_at: run.updated_at.to_rfc3339(),
        }),
    ).into_response()
}

/// POST /api/v1/payroll/runs/:id/approve
pub async fn approve_payroll_run(
    State(state): State<AppState>,
    Authed(ctx): Authed,
    Path(id): Path<Uuid>,
    Json(body): Json<ApprovePayrollRequest>,
) -> Response {
    // Signal Temporal workflow: ApprovePayroll { approver_user_id }
    // For Phase 2: update status to 'approved' in DB
    let _ = sqlx::query!(
        "UPDATE payroll_runs SET status = 'approved', updated_at = now() WHERE id = $1 AND tenant_id = $2",
        id,
        ctx.tenant_id.0
    )
    .execute(state.pool.pool())
    .await;

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "run_id": id.to_string(),
            "status": "approved",
            "approved_by": ctx.user_id.to_string(),
            "approved_at": chrono::Utc::now().to_rfc3339(),
        })),
    ).into_response()
}

/// GET /api/v1/payroll/runs/:id/payslips
pub async fn list_payslips(
    State(state): State<AppState>,
    Authed(ctx): Authed,
    Path(run_id): Path<Uuid>,
    Query(query): Query<ListPayslipsQuery>,
) -> Response {
    let page = query.page.unwrap_or(1).max(1);
    let per_page = query.per_page.unwrap_or(50).clamp(1, 200);
    let offset = (page - 1) * per_page;

    let rows = sqlx::query!(
        r#"SELECT id, payroll_run_id, employee_id, base_salary, gross_earnings, total_deductions,
                  net_pay, tax_withheld, sso_employee, sso_employer, pf_employee, pf_employer,
                  anomalies, status, created_at
           FROM payslips WHERE payroll_run_id = $1 AND tenant_id = $2
           ORDER BY created_at DESC LIMIT $3 OFFSET $4"#,
        run_id,
        ctx.tenant_id.0,
        per_page as i64,
        offset as i64
    )
    .fetch_all(state.pool.pool())
    .await
    .unwrap_or_default();

    let payslips = rows.into_iter().map(|r| PayslipSummary {
        payslip_id: r.id.to_string(),
        employee_id: r.employee_id.to_string(),
        base_salary: r.base_salary.to_string(),
        gross_earnings: r.gross_earnings.to_string(),
        net_pay: r.net_pay.to_string(),
        tax_withheld: r.tax_withheld.to_string(),
        sso_employee: r.sso_employee.to_string(),
        pf_employee: r.pf_employee.to_string(),
        status: r.status,
        anomalies: r.anomalies,
    }).collect();

    (
        StatusCode::OK,
        Json(ListPayslipsResponse { payslips, page, per_page }),
    ).into_response()
}

/// GET /api/v1/payroll/payslips/:id/pdf
pub async fn get_payslip_pdf(
    State(state): State<AppState>,
    Authed(ctx): Authed,
    Path(payslip_id): Path<Uuid>,
) -> Response {
    // Look up payslip and generate MinIO signed URL
    let row = sqlx::query!(
        "SELECT id FROM payslips WHERE id = $1 AND tenant_id = $2",
        payslip_id,
        ctx.tenant_id.0
    )
    .fetch_optional(state.pool.pool())
    .await;

    let _ = match row {
        Ok(Some(_)) => {},
        Ok(None) => return not_found(),
        Err(e) => return internal_err(e),
    };

    // TODO: Generate PDF and upload to MinIO, then return signed URL
    // For Phase 2: return placeholder
    (
        StatusCode::OK,
        Json(serde_json::json!({
            "payslip_id": payslip_id.to_string(),
            "pdf_url": format!("https://minio.example.com/talentos-files/payslips/{}.pdf", payslip_id),
            "expires_at": (chrono::Utc::now() + chrono::Duration::hours(1)).to_rfc3339(),
        })),
    ).into_response()
}

// --- DTOs ---

#[derive(Deserialize)]
pub struct StartPayrollRunRequest {
    pub period_start: NaiveDate,
    pub period_end: NaiveDate,
    pub payment_date: NaiveDate,
}

#[derive(Serialize)]
pub struct StartPayrollRunResponse {
    pub run_id: String,
    pub status: String,
    pub period_start: String,
    pub period_end: String,
    pub payment_date: String,
}

#[derive(Serialize)]
pub struct GetPayrollRunResponse {
    pub run_id: String,
    pub tenant_id: String,
    pub entity_id: Option<String>,
    pub period_start: String,
    pub period_end: String,
    pub payment_date: String,
    pub status: String,
    pub total_employees: i32,
    pub total_gross: Option<String>,
    pub total_net: Option<String>,
    pub total_tax: Option<String>,
    pub total_sso: Option<String>,
    pub total_pf: Option<String>,
    pub workflow_id: Option<String>,
    pub created_by: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Deserialize)]
pub struct ApprovePayrollRequest {
    pub reason: Option<String>,
}

#[derive(Deserialize)]
pub struct ListPayslipsQuery {
    pub page: Option<u32>,
    pub per_page: Option<u32>,
}

#[derive(Serialize)]
pub struct ListPayslipsResponse {
    pub payslips: Vec<PayslipSummary>,
    pub page: u32,
    pub per_page: u32,
}

#[derive(Serialize)]
pub struct PayslipSummary {
    pub payslip_id: String,
    pub employee_id: String,
    pub base_salary: String,
    pub gross_earnings: String,
    pub net_pay: String,
    pub tax_withheld: String,
    pub sso_employee: String,
    pub pf_employee: String,
    pub status: String,
    pub anomalies: serde_json::Value,
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
    ).into_response()
}

fn internal_err(e: sqlx::Error) -> Response {
    tracing::error!(error = %e, "database error");
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(ErrorResponse {
            error: "internal_error".into(),
            message: "An unexpected error occurred".into(),
            details: vec![],
            request_id: uuid::Uuid::new_v4().to_string(),
            retry_after_seconds: None,
        }),
    ).into_response()
}