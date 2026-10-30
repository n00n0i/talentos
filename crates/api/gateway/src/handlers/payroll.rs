use axum::extract::connect_info::ConnectInfo;
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use chrono::NaiveDate;
use serde::{Deserialize, Serialize};
use sqlx::Row;
use std::net::SocketAddr;
use uuid::Uuid;

use crate::dto::responses::ErrorResponse;
use crate::middleware::auth::{require, Authed};
use crate::state::AppState;
use talentos_hr_runner::notify::enqueue_payslip_ready;

/// POST /api/v1/payroll/runs
pub async fn start_payroll_run(
    State(state): State<AppState>,
    ConnectInfo(_addr): ConnectInfo<SocketAddr>,
    Authed(ctx): Authed,
    Json(body): Json<StartPayrollRunRequest>,
) -> Response {
    if let Err(forbidden) = require(&ctx, "payroll_run", "calculate") {
        return forbidden;
    }
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
    if let Err(forbidden) = require(&ctx, "payroll_run", "calculate") {
        return forbidden;
    }
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
    if let Err(forbidden) = require(&ctx, "payroll_run", "approve") {
        return forbidden;
    }
    let mut tx = match state.pool.begin_tenant(ctx.tenant_id).await {
        Ok(tx) => tx,
        Err(e) => return internal_err(e),
    };

    let run = sqlx::query(
        "SELECT period_start, period_end, status FROM payroll_runs WHERE id = $1 AND tenant_id = $2",
    )
    .bind(id)
    .bind(ctx.tenant_id.0)
    .fetch_optional(&mut *tx)
    .await;

    let (period_start, period_end) = match run {
        Ok(Some(row)) => {
            let status: String = row.get("status");
            if status == "approved" {
                let _ = tx.rollback().await;
                return (
                    StatusCode::OK,
                    Json(serde_json::json!({
                        "run_id": id.to_string(),
                        "status": "approved",
                        "approved_by": ctx.user_id.to_string(),
                        "approved_at": chrono::Utc::now().to_rfc3339(),
                    })),
                )
                    .into_response();
            }
            if status != "pending_approval" {
                let _ = tx.rollback().await;
                return conflict("payroll run is not ready to approve");
            }
            let s: NaiveDate = row.get("period_start");
            let e: NaiveDate = row.get("period_end");
            (s, e)
        }
        Ok(None) => {
            let _ = tx.rollback().await;
            return not_found();
        }
        Err(e) => {
            let _ = tx.rollback().await;
            return internal_err(e);
        }
    };

    let updated = sqlx::query(
        "UPDATE payroll_runs SET status = 'approved', updated_at = now() WHERE id = $1 AND tenant_id = $2 AND status = 'pending_approval'",
    )
    .bind(id)
    .bind(ctx.tenant_id.0)
    .execute(&mut *tx)
    .await;

    match updated {
        Ok(r) if r.rows_affected() == 0 => {
            let _ = tx.rollback().await;
            return conflict("payroll run is not ready to approve");
        }
        Ok(_) => {}
        Err(e) => {
            let _ = tx.rollback().await;
            return internal_err(e);
        }
    }

    if let Err(e) =
        enqueue_payslip_ready(&mut tx, ctx.tenant_id.0, id, period_start, period_end).await
    {
        let _ = tx.rollback().await;
        return internal_err(e);
    }

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

/// POST /api/v1/payroll/runs/:id/mark-paid — record the payment date/reference.
/// Only `approved → completed`; any other state is a 409.
pub async fn mark_paid(
    State(state): State<AppState>,
    Authed(ctx): Authed,
    Path(id): Path<Uuid>,
    Json(body): Json<MarkPaidRequest>,
) -> Response {
    if let Err(forbidden) = require(&ctx, "payroll_run", "approve") {
        return forbidden;
    }
    let mut tx = match state.pool.begin_tenant(ctx.tenant_id).await {
        Ok(tx) => tx,
        Err(e) => return internal_err(e),
    };

    let reference = body
        .reference
        .map(|r| r.trim().to_string())
        .filter(|r| !r.is_empty());

    let row = sqlx::query(
        "UPDATE payroll_runs SET status = 'completed', paid_at = now(), payment_reference = $3 \
         WHERE id = $1 AND tenant_id = $2 AND status = 'approved' RETURNING paid_at",
    )
    .bind(id)
    .bind(ctx.tenant_id.0)
    .bind(reference.as_deref())
    .fetch_optional(&mut *tx)
    .await;

    match row {
        Ok(Some(r)) => {
            let paid_at: chrono::DateTime<chrono::Utc> = r.get("paid_at");
            if let Err(e) = tx.commit().await {
                return internal_err(e);
            }
            (
                StatusCode::OK,
                Json(serde_json::json!({
                    "run_id": id.to_string(),
                    "status": "completed",
                    "paid_at": paid_at.to_rfc3339(),
                    "reference": reference,
                })),
            )
                .into_response()
        }
        Ok(None) => {
            let _ = tx.rollback().await;
            conflict("payroll run must be approved before marking paid")
        }
        Err(e) => {
            let _ = tx.rollback().await;
            internal_err(e)
        }
    }
}

/// GET /api/v1/payroll/runs/:id/export.csv — payroll register for accountants.
pub async fn export_payroll_csv(
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
    let run = sqlx::query(
        "SELECT period_start, period_end FROM payroll_runs WHERE id = $1 AND tenant_id = $2",
    )
    .bind(run_id)
    .bind(ctx.tenant_id.0)
    .fetch_optional(&mut *tx)
    .await;
    use chrono::NaiveDate;
    use sqlx::Row;
    let (period_start, period_end): (NaiveDate, NaiveDate) = match run {
        Ok(Some(row)) => (row.get("period_start"), row.get("period_end")),
        Ok(None) => {
            let _ = tx.rollback().await;
            return not_found();
        }
        Err(e) => {
            let _ = tx.rollback().await;
            return internal_err(e);
        }
    };

    let rows = sqlx::query(
        r#"SELECT e.employee_code, e.first_name_th, e.last_name_th,
                  p.gross_earnings, p.ot_amount, p.sso_employee, p.sso_employer,
                  p.tax_withheld, p.pf_employee, p.net_pay
             FROM payslips p
             JOIN employees e ON e.id = p.employee_id AND e.tenant_id = p.tenant_id
            WHERE p.payroll_run_id = $1 AND p.tenant_id = $2
            ORDER BY e.employee_code"#,
    )
    .bind(run_id)
    .bind(ctx.tenant_id.0)
    .fetch_all(&mut *tx)
    .await;
    let _ = tx.rollback().await;

    let rows = match rows {
        Ok(rows) => rows,
        Err(e) => return internal_err(e),
    };
    let csv_rows: Vec<CsvRow> = rows
        .iter()
        .map(|row| CsvRow {
            employee_code: row.get("employee_code"),
            employee_name: format!(
                "{} {}",
                row.get::<String, _>("first_name_th"),
                row.get::<String, _>("last_name_th")
            ),
            gross: row
                .get::<rust_decimal::Decimal, _>("gross_earnings")
                .to_string(),
            ot: row.get::<rust_decimal::Decimal, _>("ot_amount").to_string(),
            sso_employee: row
                .get::<rust_decimal::Decimal, _>("sso_employee")
                .to_string(),
            sso_employer: row
                .get::<rust_decimal::Decimal, _>("sso_employer")
                .to_string(),
            tax: row
                .get::<rust_decimal::Decimal, _>("tax_withheld")
                .to_string(),
            pf: row
                .get::<rust_decimal::Decimal, _>("pf_employee")
                .to_string(),
            net: row.get::<rust_decimal::Decimal, _>("net_pay").to_string(),
        })
        .collect();

    let body = payroll_csv(&csv_rows);
    let filename = format!("payroll-{}-{}.csv", period_start, period_end);
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

#[derive(Debug)]
struct CsvRow {
    employee_code: String,
    employee_name: String,
    gross: String,
    ot: String,
    sso_employee: String,
    sso_employer: String,
    tax: String,
    pf: String,
    net: String,
}

fn csv_field(value: &str) -> String {
    // Spreadsheet formula injection guard.
    let safe = if value.starts_with(['=', '+', '-', '@']) {
        format!("'{value}")
    } else {
        value.to_string()
    };
    format!("\"{}\"", safe.replace('"', "\"\""))
}

fn payroll_csv(rows: &[CsvRow]) -> String {
    let mut out = String::from(
        "\u{feff}employee_code,employee_name,gross_earnings,ot_amount,sso_employee,sso_employer,tax_withheld,pf_employee,net_pay\r\n",
    );
    let (mut gross, mut ot, mut sso_ee, mut sso_er, mut tax, mut pf, mut net) = (
        rust_decimal::Decimal::ZERO,
        rust_decimal::Decimal::ZERO,
        rust_decimal::Decimal::ZERO,
        rust_decimal::Decimal::ZERO,
        rust_decimal::Decimal::ZERO,
        rust_decimal::Decimal::ZERO,
        rust_decimal::Decimal::ZERO,
    );
    for row in rows {
        out.push_str(&format!(
            "{},{},{},{},{},{},{},{},{}\r\n",
            csv_field(&row.employee_code),
            csv_field(&row.employee_name),
            csv_field(&row.gross),
            csv_field(&row.ot),
            csv_field(&row.sso_employee),
            csv_field(&row.sso_employer),
            csv_field(&row.tax),
            csv_field(&row.pf),
            csv_field(&row.net),
        ));
        gross += row
            .gross
            .parse::<rust_decimal::Decimal>()
            .unwrap_or_default();
        ot += row.ot.parse::<rust_decimal::Decimal>().unwrap_or_default();
        sso_ee += row
            .sso_employee
            .parse::<rust_decimal::Decimal>()
            .unwrap_or_default();
        sso_er += row
            .sso_employer
            .parse::<rust_decimal::Decimal>()
            .unwrap_or_default();
        tax += row.tax.parse::<rust_decimal::Decimal>().unwrap_or_default();
        pf += row.pf.parse::<rust_decimal::Decimal>().unwrap_or_default();
        net += row.net.parse::<rust_decimal::Decimal>().unwrap_or_default();
    }
    out.push_str(&format!(
        "{},{},{},{},{},{},{},{},{}\r\n",
        csv_field("TOTAL"),
        csv_field(""),
        csv_field(&gross.to_string()),
        csv_field(&ot.to_string()),
        csv_field(&sso_ee.to_string()),
        csv_field(&sso_er.to_string()),
        csv_field(&tax.to_string()),
        csv_field(&pf.to_string()),
        csv_field(&net.to_string()),
    ));
    out
}

/// GET /api/v1/payroll/runs/:id/payslips
pub async fn list_payslips(
    State(state): State<AppState>,
    Authed(ctx): Authed,
    Path(run_id): Path<Uuid>,
    Query(query): Query<ListPayslipsQuery>,
) -> Response {
    if let Err(forbidden) = require(&ctx, "payslip", "view_all") {
        return forbidden;
    }
    let mut tx = match state.pool.begin_tenant(ctx.tenant_id).await {
        Ok(tx) => tx,
        Err(e) => return internal_err(e),
    };
    let page = query.page.unwrap_or(1).max(1);
    let per_page = query.per_page.unwrap_or(50).clamp(1, 200);
    let offset = (page - 1) * per_page;

    let rows = sqlx::query!(
        r#"SELECT p.id, p.payroll_run_id, p.employee_id, p.base_salary, p.gross_earnings,
                  p.total_deductions, p.net_pay, p.tax_withheld, p.sso_employee, p.sso_employer,
                  p.pf_employee, p.pf_employer, p.anomalies, p.status, p.created_at,
                  e.employee_code, e.first_name_th, e.last_name_th
           FROM payslips p
           JOIN employees e ON e.id = p.employee_id AND e.tenant_id = p.tenant_id
           WHERE p.payroll_run_id = $1 AND p.tenant_id = $2
           ORDER BY e.employee_code LIMIT $3 OFFSET $4"#,
        run_id,
        ctx.tenant_id.0,
        per_page as i64,
        offset as i64
    )
    .fetch_all(&mut *tx)
    .await
    .unwrap_or_default();

    let ids: Vec<Uuid> = rows.iter().map(|r| r.id).collect();
    let mut acks: std::collections::HashMap<Uuid, chrono::DateTime<chrono::Utc>> =
        std::collections::HashMap::new();
    if !ids.is_empty() {
        if let Ok(ack_rows) = sqlx::query(
            "SELECT payslip_id, acknowledged_at FROM payslip_acknowledgements \
             WHERE tenant_id = $1 AND payslip_id = ANY($2)",
        )
        .bind(ctx.tenant_id.0)
        .bind(&ids)
        .fetch_all(&mut *tx)
        .await
        {
            use sqlx::Row;
            for row in ack_rows {
                if let (Ok(id), Ok(ts)) = (
                    row.try_get::<Uuid, _>("payslip_id"),
                    row.try_get::<chrono::DateTime<chrono::Utc>, _>("acknowledged_at"),
                ) {
                    acks.insert(id, ts);
                }
            }
        }
    }

    let payslips = rows
        .into_iter()
        .map(|r| PayslipSummary {
            acknowledged_at: acks.get(&r.id).map(|t| t.to_rfc3339()),
            payslip_id: r.id.to_string(),
            employee_id: r.employee_id.to_string(),
            employee_code: r.employee_code,
            employee_name: format!("{} {}", r.first_name_th, r.last_name_th),
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

/// GET /api/v1/payroll/payslips/:id/pdf — real PDF (Chromium, Thai shaping).
pub async fn get_payslip_pdf(
    State(state): State<AppState>,
    Authed(ctx): Authed,
    Path(payslip_id): Path<Uuid>,
) -> Response {
    if let Err(forbidden) = require(&ctx, "payslip", "view_all") {
        return forbidden;
    }
    let html = match talentos_payroll_runner::render_payslip(&state.pool, ctx.tenant_id, payslip_id)
        .await
    {
        Ok(html) => html,
        Err(talentos_payroll_runner::RunError::NotFound(_)) => return not_found(),
        Err(e) => {
            tracing::error!(error = %e, "payslip html render failed");
            return internal_err_msg("payslip could not be rendered");
        }
    };

    let cfg = talentos_payroll_runner::pdf::PdfConfig::default();
    match talentos_payroll_runner::pdf::get_or_render_pdf(&cfg, ctx.tenant_id.0, payslip_id, &html)
        .await
    {
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
            tracing::error!(error = %e, "payslip pdf failed");
            internal_err_msg("payslip pdf could not be generated")
        }
    }
}

/// GET /api/v1/payroll/payslips/:id/render — printable payslip (text/html).
/// Uses the tenant's payslip template; `/pdf` is kept as an alias so existing
/// clients keep working until a real PDF pipeline lands.
pub async fn render_payslip(
    State(state): State<AppState>,
    Authed(ctx): Authed,
    Path(payslip_id): Path<Uuid>,
) -> Response {
    if let Err(forbidden) = require(&ctx, "payslip", "view_all") {
        return forbidden;
    }
    match talentos_payroll_runner::render_payslip(&state.pool, ctx.tenant_id, payslip_id).await {
        Ok(html) => (
            StatusCode::OK,
            [(axum::http::header::CONTENT_TYPE, "text/html; charset=utf-8")],
            html,
        )
            .into_response(),
        Err(talentos_payroll_runner::RunError::NotFound(_)) => not_found(),
        Err(e) => {
            tracing::error!(error = %e, "payslip render failed");
            internal_err_msg("payslip could not be rendered")
        }
    }
}

/// GET /api/v1/payslip/template — current tenant template (defaults when unset).
pub async fn get_template(State(state): State<AppState>, Authed(ctx): Authed) -> Response {
    if let Err(forbidden) = require(&ctx, "settings", "view_company") {
        return forbidden;
    }
    match talentos_payroll_runner::load_template(&state.pool, ctx.tenant_id).await {
        Ok(t) => (StatusCode::OK, Json(t)).into_response(),
        Err(e) => {
            tracing::error!(error = %e, "load template failed");
            internal_err_msg("template could not be loaded")
        }
    }
}

/// PUT /api/v1/payslip/template — upsert the default template.
/// Validation returns 422 so the UI can point at the offending field.
pub async fn put_template(
    State(state): State<AppState>,
    Authed(ctx): Authed,
    Json(template): Json<talentos_payroll::render::PayslipTemplate>,
) -> Response {
    if let Err(forbidden) = require(&ctx, "settings", "manage_company") {
        return forbidden;
    }
    match talentos_payroll_runner::save_template(&state.pool, ctx.tenant_id, &template).await {
        Ok(()) => (StatusCode::OK, Json(template)).into_response(),
        Err(talentos_payroll_runner::RunError::InvalidTemplate(msg)) => (
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
        Err(e) => {
            tracing::error!(error = %e, "save template failed");
            internal_err_msg("template could not be saved")
        }
    }
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
pub struct MarkPaidRequest {
    /// Optional payment reference (e.g. batch id); stored verbatim.
    pub reference: Option<String>,
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
    pub employee_code: String,
    pub employee_name: String,
    pub base_salary: String,
    pub gross_earnings: String,
    pub net_pay: String,
    pub tax_withheld: String,
    pub sso_employee: String,
    pub pf_employee: String,
    pub status: String,
    pub anomalies: serde_json::Value,
    pub acknowledged_at: Option<String>,
}

fn conflict(message: &str) -> Response {
    (
        StatusCode::CONFLICT,
        Json(ErrorResponse {
            error: "conflict".into(),
            message: message.to_string(),
            details: vec![],
            request_id: Uuid::new_v4().to_string(),
            retry_after_seconds: None,
        }),
    )
        .into_response()
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

#[cfg(test)]
mod csv_tests {
    use super::*;

    fn row(code: &str, name: &str, net: &str) -> CsvRow {
        CsvRow {
            employee_code: code.into(),
            employee_name: name.into(),
            gross: "41000.00".into(),
            ot: "1000.00".into(),
            sso_employee: "750.00".into(),
            sso_employer: "750.00".into(),
            tax: "1975.00".into(),
            pf: "0.00".into(),
            net: net.into(),
        }
    }

    #[test]
    fn csv_has_bom_header_rows_and_totals() {
        let out = payroll_csv(&[row("E001", "สมชาย ทดสอบ", "38275.00")]);
        assert!(out.starts_with('\u{feff}'));
        assert!(out.contains("employee_code,employee_name"));
        assert!(out.contains("\"E001\",\"สมชาย ทดสอบ\""));
        assert!(out.contains("\"TOTAL\""));
        assert!(out.contains("\"38275.00\""));
        assert!(out.ends_with("\r\n"));
    }

    #[test]
    fn csv_neutralizes_formula_injection() {
        assert_eq!(csv_field("=HYPERLINK(\"x\")"), "\"'=HYPERLINK(\"\"x\"\")\"");
        assert_eq!(csv_field("normal"), "\"normal\"");
    }
}
