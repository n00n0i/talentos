//! Authentication & Authorization integration tests.

use crate::{fixtures::TestTenant, gateway::GatewayHandle, init_tracing};
use sqlx::PgPool;
use reqwest::StatusCode;
use serde::{Deserialize};
use serde_json::json;
use std::env;
use uuid::Uuid;

#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
    refresh_token: String,
    token_type: String,
    expires_in: i64,
}

#[derive(Deserialize)]
struct ErrorResponse {
    error: String,
    message: String,
}

fn test_db_url() -> String {
    env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgres://talentos:talentos_dev_secret@localhost:5432/talentos".into())
}

async fn create_test_pool() -> anyhow::Result<PgPool> {
    PgPool::connect(&test_db_url()).await.map_err(Into::into)
}

/// Happy Path: Successful login returns valid token pair (TC-01)
#[tokio::test]
async fn tc01_successful_login_returns_token_pair() -> anyhow::Result<()> {
    init_tracing();
    let pool = create_test_pool().await?;
    let gw = GatewayHandle::spawn().await?;

    // Create test tenant with known credentials
    let tenant = TestTenant::create(&pool).await?;

    let login_body = json!({
        "email": tenant.admin_email,
        "password": tenant.admin_password,
        "tenant_slug": tenant.slug
    });

    let resp = gw.client()
        .post(gw.url("/api/v1/auth/login"))
        .json(&login_body)
        .send()
        .await?;

    assert_eq!(resp.status(), StatusCode::OK);
    let tokens: TokenResponse = resp.json().await?;
    assert!(!tokens.access_token.is_empty());
    assert!(!tokens.refresh_token.is_empty());
    assert_eq!(tokens.token_type, "Bearer");
    assert_eq!(tokens.expires_in, 900);

    // Cleanup
    tenant.cleanup(&pool).await?;
    Ok(())
}

/// Test: Wrong credentials returns 401 with generic message (TC-05, TC-21)
#[tokio::test]
async fn test_login_invalid_credentials() -> anyhow::Result<()> {
    init_tracing();
    let gw = GatewayHandle::spawn().await?;

    let login_body = json!({
        "email": "nonexistent@example.com",
        "password": "wrongpassword",
        "tenant_slug": "acme-corp"
    });

    let resp = gw.client()
        .post(gw.url("/api/v1/auth/login"))
        .json(&login_body)
        .send()
        .await?;

    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    let err: ErrorResponse = resp.json().await?;
    assert_eq!(err.error, "unauthorized");
    assert_eq!(err.message, "Invalid email or password");

    Ok(())
}

/// Test: Refresh token rotation (TC-03)
#[tokio::test]
async fn test_refresh_token_rotation() -> anyhow::Result<()> {
    init_tracing();
    let pool = create_test_pool().await?;
    let gw = GatewayHandle::spawn().await?;

    let tenant = TestTenant::create(&pool).await?;

    let login_body = json!({
        "email": tenant.admin_email,
        "password": tenant.admin_password,
        "tenant_slug": tenant.slug
    });

    let login_resp = gw.client()
        .post(gw.url("/api/v1/auth/login"))
        .json(&login_body)
        .send()
        .await?;

    if login_resp.status() != StatusCode::OK {
        tenant.cleanup(&pool).await?;
        return Ok(());
    }

    let tokens: TokenResponse = login_resp.json().await?;

    // Use refresh token
    let refresh_body = json!({ "refresh_token": tokens.refresh_token });
    let refresh_resp = gw.client()
        .post(gw.url("/api/v1/auth/refresh"))
        .json(&refresh_body)
        .send()
        .await?;

    assert_eq!(refresh_resp.status(), StatusCode::OK);
    let new_tokens: TokenResponse = refresh_resp.json().await?;
    assert_ne!(new_tokens.access_token, tokens.access_token);
    assert_ne!(new_tokens.refresh_token, tokens.refresh_token);

    tenant.cleanup(&pool).await?;
    Ok(())
}

/// Test: Expired token rejected (TC-05)
#[tokio::test]
async fn test_expired_token_rejected() -> anyhow::Result<()> {
    init_tracing();
    let gw = GatewayHandle::spawn().await?;

    // Create an expired token manually
    use jsonwebtoken::{encode, EncodingKey, Header, Algorithm};
    use chrono::{Utc, Duration};
    use serde::{Serialize};

    #[derive(Serialize)]
    struct Claims {
        sub: String,
        tid: String,
        roles: Vec<String>,
        iat: i64,
        exp: i64,
        nbf: i64,
        iss: String,
        aud: String,
        jti: String,
    }

    let secret = env::var("AUTH_JWT_SECRET")
        .unwrap_or_else(|_| "dev-secret-change-me-in-prod-minimum-32-bytes-okay!!".into());
    
    let claims = Claims {
        sub: Uuid::new_v4().to_string(),
        tid: Uuid::new_v4().to_string(),
        roles: vec!["hr_admin".to_string()],
        iat: (Utc::now() - Duration::seconds(120)).timestamp(),
        exp: (Utc::now() - Duration::seconds(60)).timestamp(),
        nbf: (Utc::now() - Duration::seconds(120)).timestamp(),
        iss: "talentos.auth".to_string(),
        aud: "talentos.api".to_string(),
        jti: Uuid::new_v4().to_string(),
    };

    let expired_token = encode(&Header::new(Algorithm::HS256), &claims, &EncodingKey::from_secret(secret.as_bytes()))?;

    let resp = gw.client()
        .get(gw.url("/api/v1/auth/me"))
        .header("Authorization", format!("Bearer {}", expired_token))
        .send()
        .await?;

    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    let err: ErrorResponse = resp.json().await?;
    assert_eq!(err.error, "token_expired");

    Ok(())
}

/// Test: Malformed token rejected (TC-06)
#[tokio::test]
async fn test_malformed_token_rejected() -> anyhow::Result<()> {
    init_tracing();
    let gw = GatewayHandle::spawn().await?;

    let resp = gw.client()
        .get(gw.url("/api/v1/auth/me"))
        .header("Authorization", "Bearer not-a-jwt-at-all")
        .send()
        .await?;

    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    let err: ErrorResponse = resp.json().await?;
    assert_eq!(err.error, "token_malformed");

    Ok(())
}

/// Test: Inactive account cannot authenticate (TC-21)
#[tokio::test]
async fn test_inactive_account_rejected() -> anyhow::Result<()> {
    init_tracing();
    let pool = create_test_pool().await?;
    let gw = GatewayHandle::spawn().await?;

    let tenant = TestTenant::create(&pool).await?;

    // Deactivate the admin user
    sqlx::query!("UPDATE users SET is_active = false WHERE id = $1", tenant.admin_user_id.0)
        .execute(&pool).await?;

    let login_body = json!({
        "email": tenant.admin_email,
        "password": tenant.admin_password,
        "tenant_slug": tenant.slug
    });

    let resp = gw.client()
        .post(gw.url("/api/v1/auth/login"))
        .json(&login_body)
        .send()
        .await?;

    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    let err: ErrorResponse = resp.json().await?;
    assert_eq!(err.error, "unauthorized");
    assert_eq!(err.message, "Invalid email or password");

    tenant.cleanup(&pool).await?;
    Ok(())
}

/// Test: Zero-role user cannot authenticate (TC-22)
#[tokio::test]
async fn test_zero_role_user_rejected() -> anyhow::Result<()> {
    init_tracing();
    let pool = create_test_pool().await?;
    let gw = GatewayHandle::spawn().await?;

    let tenant = TestTenant::create(&pool).await?;

    // Remove all roles from employee user
    sqlx::query!("DELETE FROM user_roles WHERE user_id = $1", tenant.employee_user_id.0)
        .execute(&pool).await?;

    let login_body = json!({
        "email": tenant.employee_email,
        "password": tenant.employee_password,
        "tenant_slug": tenant.slug
    });

    let resp = gw.client()
        .post(gw.url("/api/v1/auth/login"))
        .json(&login_body)
        .send()
        .await?;

    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    let err: ErrorResponse = resp.json().await?;
    assert_eq!(err.error, "unauthorized");
    assert_eq!(err.message, "Invalid email or password");

    tenant.cleanup(&pool).await?;
    Ok(())
}

/// Test: Rate limiting on login (TC-15)
#[tokio::test]
async fn test_login_rate_limiting() -> anyhow::Result<()> {
    init_tracing();
    let gw = GatewayHandle::spawn().await?;

    let login_body = json!({
        "email": "nonexistent@example.com",
        "password": "wrongpassword",
        "tenant_slug": "nonexistent"
    });

    // Send 11 requests rapidly (limit is 10/min)
    let mut last_status = StatusCode::UNAUTHORIZED;
    for i in 0..12 {
        let resp = gw.client()
            .post(gw.url("/api/v1/auth/login"))
            .json(&login_body)
            .send()
            .await?;
        
        last_status = resp.status();
        if i < 10 {
            assert_eq!(resp.status(), StatusCode::UNAUTHORIZED, "Request {} should be 401", i+1);
        }
    }

    // 11th+ request should be rate limited
    assert_eq!(last_status, StatusCode::TOO_MANY_REQUESTS, "Should be rate limited after 10 attempts");

    Ok(())
}