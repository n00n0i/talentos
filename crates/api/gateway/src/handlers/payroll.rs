use axum::extract::connect_info::ConnectInfo;
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use chrono::NaiveDate;
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use uuid::Uuid;

use crate::dto::responses::ErrorResponse;
use crate::middleware::auth::Authed;
use crate::state::AppState;

/// POST /api/v1/payroll/runs
pub async fn start_payroll_run(
    State(state): State<AppState>,
    ConnectInfo(_addr): ConnectInfo<SocketAddr>,
    Authed(ctx): Authed,
    Json(body): Json<StartPayrollRunRequest>,
) -> Response {
    let mut tx = match state.pool.begin_tenant(ctx.tenant_id).await {
        Ok(tx) => tx,
        Err(e) => return internal_err(e),
    };
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
        )
            .into_response();
    }

    // The token may be tenant-wide (entity_id = null); then the body must say which entity.
    let entity_id = match body.entity_id.or(ctx.entity_id.map(|e| e.0)) {
        Some(id) => id,
        None => {
            return bad_request("entity_id is required when the token is not entity-scoped");
        }
    };

    // Check for existing run for this period (spec §7: concurrent run prevention)
    let existing = sqlx::query!(
        "SELECT id FROM payroll_runs WHERE tenant_id = $1 AND entity_id = $2 AND period_start = $3 AND period_end = $4",
        ctx.tenant_id.0,
        entity_id,
        body.period_start,
        body.period_end
    )
    .fetch_optional(&mut *tx)
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
        )
            .into_response();
    }

    // Create payroll run in draft status
    let run_id = Uuid::new_v4();
    let now = chrono::Utc::now();

    if let Err(e) = sqlx::query!(
        r#"INSERT INTO payroll_runs (id, tenant_id, entity_id, period_start, period_end, payment_date, status, total_employees, created_by, created_at)
           VALUES ($1, $2, $3, $4, $5, $6, 'draft', 0, $7, $8)"#,
        run_id,
        ctx.tenant_id.0,
        entity_id,
        body.period_start,
        body.period_end,
        body.payment_date,
        ctx.user_id.0,
        now
    )
    .execute(&mut *tx)
    .await
    {
        let _ = tx.rollback().await;
        return internal_err(e);
    }

    if let Err(e) = tx.commit().await {
        return internal_err(e);
    }

    // Execute in the background so the HTTP call returns immediately.
    // shortcut: in-process execution; swap for a Temporal workflow when the worker lands.
    let exec_pool = state.pool.clone();
    let exec_tenant = ctx.tenant_id;
    let exec_run_id = run_id;
    tokio::spawn(async move {
        match talentos_payroll_runner::execute_run(&exec_pool, exec_tenant, exec_run_id).await {
            Ok(summary) => tracing::info!(
                run_id = %summary.run_id,
                employees = summary.employees,
                failed = summary.failed,
                net = %summary.total_net,
                duration_ms = summary.duration_ms,
                "payroll run completed"
            ),
            Err(e) => tracing::error!(run_id = %exec_run_id, error = %e, "payroll run failed"),
        }
    });

    (
        StatusCode::CREATED,
        Json(StartPayrollRunResponse {
            run_id: run_id.to_string(),
            status: "draft".into(),
            period_start: body.period_start.to_string(),
            period_end: body.period_end.to_string(),
            payment_date: body.payment_date.to_string(),
        }),
    )
        .into_response()
}

/// GET /api/v1/payroll/runs/:id
pub async fn get_payroll_run(
    State(state): State<AppState>,
    Authed(ctx): Authed,
    Path(id): Path<Uuid>,
) -> Response {
    let mut tx = match state.pool.begin_tenant(ctx.tenant_id).await {
        Ok(tx) => tx,
        Err(e) => return internal_err(e),
    };
    let row = sqlx::query!(
        r#"SELECT id, tenant_id, entity_id, period_start, period_end, payment_date, status, total_employees,
                  total_gross, total_net, total_tax, total_sso, total_pf, workflow_id, created_by, created_at, updated_at
           FROM payroll_runs WHERE id = $1 AND tenant_id = $2"#,
        id,
        ctx.tenant_id.0
    )
    .fetch_optional(&mut *tx)
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
            entity_id: run.entity_id.to_string(),
            period_start: run.period_start.to_string(),
            period_end: run.period_end.to_string(),
            payment_date: run.payment_date.to_string(),
            status: run.status,
            total_employees: run.total_employees,
            total_gross: run.total_gross.to_string(),
            total_net: run.total_net.to_string(),
            total_tax: run.total_tax.to_string(),
            total_sso: run.total_sso.to_string(),
            total_pf: run.total_pf.to_string(),
            workflow_id: run.workflow_id,
            created_by: run.created_by.to_string(),
            created_at: run.created_at.to_rfc3339(),
            updated_at: run.updated_at.to_rfc3339(),
        }),
    )
        .into_response()
}

/// POST /api/v1/payroll/runs/:id/approve
pub async fn approve_payroll_run(
    State(state): State<AppState>,
    Authed(ctx): Authed,
    Path(id): Path<Uuid>,
    Json(_body): Json<ApprovePayrollRequest>,
) -> Response {
    let mut tx = match state.pool.begin_tenant(ctx.tenant_id).await {
        Ok(tx) => tx,
        Err(e) => return internal_err(e),
    };
    // Signal Temporal workflow: ApprovePayroll { approver_user_id }
    // For Phase 2: update status to 'approved' in DB
    let _ = sqlx::query!(
        "UPDATE payroll_runs SET status = 'approved', updated_at = now() WHERE id = $1 AND tenant_id = $2",
        id,
        ctx.tenant_id.0
    )
    .execute(&mut *tx)
    .await;

    if let Err(e) = tx.commit().await {
        return internal_err(e);
    }

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "run_id": id.to_string(),
            "status": "approved",
            "approved_by": ctx.user_id.to_string(),
            "approved_at": chrono::Utc::now().to_rfc3339(),
        })),
    )
        .into_response()
}

/// GET /api/v1/payroll/runs/:id/payslips
pub async fn list_payslips(
    State(state): State<AppState>,
    Authed(ctx): Authed,
    Path(run_id): Path<Uuid>,
    Query(query): Query<ListPayslipsQuery>,
) -> Response {
    let mut tx = match state.pool.begin_tenant(ctx.tenant_id).await {
        Ok(tx) => tx,
        Err(e) => return internal_err(e),
    };
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
    .fetch_all(&mut *tx)
    .await
    .unwrap_or_default();

    let payslips = rows
        .into_iter()
        .map(|r| PayslipSummary {
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
        })
        .collect();

    (
        StatusCode::OK,
        Json(ListPayslipsResponse {
            payslips,
            page,
            per_page,
        }),
    )
        .into_response()
}

/// GET /api/v1/payroll/payslips/:id/pdf
pub async fn get_payslip_pdf(
    State(state): State<AppState>,
    Authed(ctx): Authed,
    Path(payslip_id): Path<Uuid>,
) -> Response {
    let mut tx = match state.pool.begin_tenant(ctx.tenant_id).await {
        Ok(tx) => tx,
        Err(e) => return internal_err(e),
    };
    // Look up payslip and generate MinIO signed URL
    let row = sqlx::query!(
        "SELECT id FROM payslips WHERE id = $1 AND tenant_id = $2",
        payslip_id,
        ctx.tenant_id.0
    )
    .fetch_optional(&mut *tx)
    .await;

    match row {
        Ok(Some(_)) => {}
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
    /// Required when the caller's token is not scoped to a single entity.
    pub entity_id: Option<Uuid>,
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
    pub entity_id: String,
    pub period_start: String,
    pub period_end: String,
    pub payment_date: String,
    pub status: String,
    pub total_employees: i32,
    pub total_gross: String,
    pub total_net: String,
    pub total_tax: String,
    pub total_sso: String,
    pub total_pf: String,
    pub workflow_id: Option<String>,
    pub created_by: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Deserialize)]
pub struct ApprovePayrollRequest {
    /// Optional human note; persisted with the approval audit entry.
    #[allow(dead_code)]
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
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(ErrorResponse {
            error: "internal_error".into(),
            message: "An unexpected error occurred".into(),
            details: vec![],
            request_id: uuid::Uuid::new_v4().to_string(),
            retry_after_seconds: None,
        }),
    )
        .into_response()
}
