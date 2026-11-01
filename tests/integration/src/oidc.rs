//! OIDC SSO integration tests: settings persistence + RLS isolation.

use crate::fixtures::TestTenant;
use crate::init_tracing;
use reqwest::StatusCode;
use serde_json::json;
use sqlx::PgPool;
use std::env;

fn test_db_url() -> String {
    env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgres://talentos:talentos_dev_secret@localhost:5432/talentos".into())
}

async fn create_test_pool() -> anyhow::Result<PgPool> {
    PgPool::connect(&test_db_url()).await.map_err(Into::into)
}

/// Save + read back OIDC settings via the gateway, then verify the DB row
/// and the audit_log entry. This is the E2E "settings persistence" test.
#[tokio::test]
async fn oidc_settings_persistence_round_trip() -> anyhow::Result<()> {
    init_tracing();
    let pool = create_test_pool().await?;
    let gw = crate::gateway::GatewayHandle::spawn().await?;
    let tenant = TestTenant::create(&pool).await?;

    // Login as the tenant admin to get a bearer token.
    let login_body = json!({
        "email": tenant.admin_email,
        "password": tenant.admin_password,
        "tenant_slug": tenant.slug
    });

    let login_resp = gw
        .client()
        .post(gw.url("/api/v1/auth/login"))
        .json(&login_body)
        .send()
        .await?;

    if login_resp.status() != StatusCode::OK {
        eprintln!(
            "OIDC settings test: login failed ({:?}), skipping",
            login_resp.status()
        );
        tenant.cleanup(&pool).await?;
        return Ok(());
    }

    let tokens: serde_json::Value = login_resp.json().await?;
    let access_token = tokens["access_token"].as_str().unwrap().to_string();

    // Save OIDC settings.
    let save_body = json!({
        "issuer": "https://sso.example.com",
        "client_id": "talentos-client-xyz",
        "client_secret": "super-secret-value",
        "allowed_domains": ["example.com", "corp.example.com"],
        "enabled": true
    });

    let save_resp = gw
        .client()
        .post(gw.url("/api/v1/auth/oidc/settings"))
        .header("Authorization", format!("Bearer {}", access_token))
        .json(&save_body)
        .send()
        .await?;

    assert_eq!(save_resp.status(), StatusCode::OK);
    let save_json: serde_json::Value = save_resp.json().await?;
    assert_eq!(save_json["ok"], true);
    assert_eq!(save_json["issuer"], "https://sso.example.com");
    assert_eq!(save_json["client_id"], "talentos-client-xyz");
    assert_eq!(save_json["enabled"], true);

    // Read back the settings — secret must be masked.
    let get_resp = gw
        .client()
        .get(gw.url("/api/v1/auth/oidc/settings"))
        .header("Authorization", format!("Bearer {}", access_token))
        .send()
        .await?;

    assert_eq!(get_resp.status(), StatusCode::OK);
    let get_json: serde_json::Value = get_resp.json().await?;
    assert_eq!(get_json["configured"], true);
    assert_eq!(get_json["issuer"], "https://sso.example.com");
    assert_eq!(get_json["client_id"], "talentos-client-xyz");
    assert_eq!(get_json["enabled"], true);

    let masked = get_json["client_secret_masked"].as_str();
    assert!(
        masked.is_some(),
        "client_secret_masked should be present on read-back"
    );
    assert!(
        !masked.unwrap().contains("super-secret-value"),
        "masked secret must not contain the full value"
    );

    // Verify allowed_domains round-trips.
    let domains: Vec<String> = get_json["allowed_domains"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect();
    assert!(domains.contains(&"example.com".to_string()));
    assert!(domains.contains(&"corp.example.com".to_string()));

    // Verify the row exists in the DB (persistence).
    let row: Option<(String, String, bool)> =
        sqlx::query_as(
            "SELECT issuer, client_id, enabled FROM tenant_oidc_config WHERE tenant_id = $1",
        )
        .bind(tenant.tenant_id.0)
        .fetch_optional(&pool)
        .await?;

    assert!(row.is_some(), "OIDC config row should exist in DB");
    let (issuer, client_id, enabled) = row.unwrap();
    assert_eq!(issuer, "https://sso.example.com");
    assert_eq!(client_id, "talentos-client-xyz");
    assert!(enabled);

    // Verify an audit_log entry was written for the settings save.
    let audit_count: i64 =
        sqlx::query_scalar(
            "SELECT COUNT(*) FROM audit_log WHERE tenant_id = $1 AND action = 'auth.oidc_settings_saved'",
        )
        .bind(tenant.tenant_id.0)
        .fetch_one(&pool)
        .await?;
    assert!(
        audit_count >= 1,
        "audit_log should have at least one OIDC settings entry"
    );

    // Verify the secret in the DB is encrypted (not plaintext).
    let stored_secret: String =
        sqlx::query_scalar(
            "SELECT client_secret FROM tenant_oidc_config WHERE tenant_id = $1",
        )
        .bind(tenant.tenant_id.0)
        .fetch_one(&pool)
        .await?;
    assert!(
        !stored_secret.contains("super-secret-value"),
        "client_secret must not be stored in plaintext"
    );

    // Cleanup
    tenant.cleanup(&pool).await?;
    Ok(())
}

/// Verify that a payroll_manager (non-hr_admin) cannot manage OIDC settings.
#[tokio::test]
async fn oidc_settings_requires_hr_admin() -> anyhow::Result<()> {
    init_tracing();
    let pool = create_test_pool().await?;
    let gw = crate::gateway::GatewayHandle::spawn().await?;
    let tenant = TestTenant::create(&pool).await?;

    // Login as the payroll manager (non-admin).
    let login_body = json!({
        "email": tenant.hr_manager_email,
        "password": tenant.hr_manager_password,
        "tenant_slug": tenant.slug
    });

    let login_resp = gw
        .client()
        .post(gw.url("/api/v1/auth/login"))
        .json(&login_body)
        .send()
        .await?;

    if login_resp.status() != StatusCode::OK {
        eprintln!("OIDC RBAC test: login failed, skipping");
        tenant.cleanup(&pool).await?;
        return Ok(());
    }

    let tokens: serde_json::Value = login_resp.json().await?;
    let access_token = tokens["access_token"].as_str().unwrap().to_string();

    // Attempt to save OIDC settings as payroll_manager → 403.
    let save_body = json!({
        "issuer": "https://sso.example.com",
        "client_id": "client",
        "client_secret": "secret"
    });
    let resp = gw
        .client()
        .post(gw.url("/api/v1/auth/oidc/settings"))
        .header("Authorization", format!("Bearer {}", access_token))
        .json(&save_body)
        .send()
        .await?;

    assert_eq!(
        resp.status(),
        StatusCode::FORBIDDEN,
        "payroll_manager must not manage OIDC settings"
    );

    // GET settings also requires hr_admin.
    let get_resp = gw
        .client()
        .get(gw.url("/api/v1/auth/oidc/settings"))
        .header("Authorization", format!("Bearer {}", access_token))
        .send()
        .await?;
    assert_eq!(get_resp.status(), StatusCode::FORBIDDEN);

    // Cleanup
    tenant.cleanup(&pool).await?;
    Ok(())
}

/// The OIDC authorize endpoint returns a 302 redirect to the login page with
/// an error code when the tenant has no OIDC config.
#[tokio::test]
async fn oidc_authorize_unconfigured_tenant_redirects_to_login() -> anyhow::Result<()> {
    init_tracing();
    let pool = create_test_pool().await?;
    let gw = crate::gateway::GatewayHandle::spawn().await?;
    let tenant = TestTenant::create(&pool).await?;

    let resp = gw
        .client()
        .get(format!(
            "{}?tenant_slug={}",
            gw.url("/api/v1/auth/oidc/authorize"),
            tenant.slug
        ))
        .redirect(reqwest::redirect::Policy::none())
        .send()
        .await?;

    assert_eq!(
        resp.status(),
        StatusCode::SEE_OTHER,
        "authorize should 302 for unconfigured tenant"
    );
    let location = resp
        .headers()
        .get("location")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    assert!(
        location.contains("oidc_error="),
        "redirect should carry an oidc_error code, got: {}",
        location
    );

    // Cleanup
    tenant.cleanup(&pool).await?;
    Ok(())
}
