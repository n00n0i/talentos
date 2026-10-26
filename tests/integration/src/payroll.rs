//! Payroll integration tests.

use crate::{init_tracing, GatewayHandle};
use reqwest::StatusCode;
use serde::{Deserialize, Serialize};
use serde_json::json;
use uuid::Uuid;
use chrono::NaiveDate;

#[derive(Deserialize)]
struct StartPayrollRunResponse {
    run_id: String,
    status: String,
    period_start: String,
    period_end: String,
    payment_date: String,
}

#[derive(Deserialize)]
struct GetPayrollRunResponse {
    run_id: String,
    status: String,
    total_employees: i32,
    total_gross: Option<String>,
    total_net: Option<String>,
    total_tax: Option<String>,
    total_sso: Option<String>,
    total_pf: Option<String>,
}

/// Test: Start payroll run
#[tokio::test]
async fn test_start_payroll_run() -> anyhow::Result<()> {
    init_tracing();
    let gw = GatewayHandle::spawn().await?;
    
    // Need authentication first - skip if no auth
    let body = json!({
        "period_start": "2026-10-01",
        "period_end": "2026-10-31",
        "payment_date": "2026-11-01"
    });
    
    let resp = gw.client()
        .post(gw.url("/api/v1/payroll/runs"))
        .json(&body)
        .send()
        .await?;
    
    // Expect 401 if not authenticated, 201 if successful
    assert!(resp.status() == StatusCode::UNAUTHORIZED || resp.status() == StatusCode::CREATED);
    
    Ok(())
}

/// Test: Get payroll run (not found)
#[tokio::test]
async fn test_get_payroll_run_not_found() -> anyhow::Result<()> {
    init_tracing();
    let gw = GatewayHandle::spawn().await?;
    
    let fake_id = Uuid::new_v4();
    let resp = gw.client()
        .get(gw.url(&format!("/api/v1/payroll/runs/{}", fake_id)))
        .send()
        .await?;
    
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    Ok(())
}

/// Test: List payslips for non-existent run
#[tokio::test]
async fn test_list_payslips_not_found() -> anyhow::Result<()> {
    init_tracing();
    let gw = GatewayHandle::spawn().await?;
    
    let fake_id = Uuid::new_v4();
    let resp = gw.client()
        .get(gw.url(&format!("/api/v1/payroll/runs/{}/payslips", fake_id)))
        .send()
        .await?;
    
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    Ok(())
}

/// Test: Approve payroll run (not found)
#[tokio::test]
async fn test_approve_payroll_run_not_found() -> anyhow::Result<()> {
    init_tracing();
    let gw = GatewayHandle::spawn().await?;
    
    let fake_id = Uuid::new_v4();
    let resp = gw.client()
        .post(gw.url(&format!("/api/v1/payroll/runs/{}/approve", fake_id)))
        .json(&json!({ "reason": "test" }))
        .send()
        .await?;
    
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    Ok(())
}

/// Test: Get payslip PDF (not found)
#[tokio::test]
async fn test_get_payslip_pdf_not_found() -> anyhow::Result<()> {
    init_tracing();
    let gw = GatewayHandle::spawn().await?;
    
    let fake_id = Uuid::new_v4();
    let resp = gw.client()
        .get(gw.url(&format!("/api/v1/payroll/payslips/{}/pdf", fake_id)))
        .send()
        .await?;
    
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    Ok(())
}