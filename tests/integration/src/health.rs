//! Health endpoint integration tests.

use crate::{init_tracing, GatewayHandle};
use reqwest::StatusCode;
use serde_json::Value;

#[tokio::test]
async fn test_health_endpoint_returns_ok() -> anyhow::Result<()> {
    init_tracing();
    let gw = GatewayHandle::spawn().await?;
    
    let resp = gw.client().get(gw.url("/health")).send().await?;
    assert_eq!(resp.status(), StatusCode::OK);
    
    let body: serde_json::Value = resp.json().await?;
    assert_eq!(body["status"], "ok");
    assert!(body["version"].is_string());
    assert!(body["checks"]["database"].is_string());
    assert!(body["checks"]["redis"].is_string());
    assert!(body["uptime_seconds"].is_number());
    
    Ok(())
}

#[tokio::test]
async fn test_health_endpoint_no_auth_required() -> anyhow::Result<()> {
    init_tracing();
    let gw = GatewayHandle::spawn().await?;
    
    // No Authorization header needed
    let resp = gw.client().get(gw.url("/health")).send().await?;
    assert_eq!(resp.status(), StatusCode::OK);
    
    Ok(())
}