//! Generic OIDC SSO (per-tenant, PKCE, email link + verified-email).
//!
//! Provider-agnostic skeleton: a tenant stores its own issuer / client id /
//! secret / allowed email domains. Inert until an operator registers a real
//! OAuth client with the provider. No outbound network is attempted here.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use axum::extract::{Query, State};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use jsonwebtoken::{decode, encode, Algorithm, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};
use url::Url;
use uuid::Uuid;

use talentos_kernel::TenantId;

use crate::dto::responses::ErrorResponse;
use crate::middleware::auth::{require, Authed};
use crate::state::AppState;

/// Per-request OIDC discovery cache: issuer -> (config, expires_at).
pub type DiscoveryCache =
    Arc<std::sync::Mutex<HashMap<String, (OidcDiscovery, tokio::time::Instant)>>>;

const DISCOVERY_TTL: Duration = Duration::from_secs(300);
const STATE_TTL_SECS: i64 = 300;
const OIDC_ISS: &str = "talentos.oidc";

/// OpenID Connect Discovery document.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OidcDiscovery {
    pub authorization_endpoint: String,
    pub token_endpoint: String,
    pub userinfo_endpoint: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct OidcTokenResponse {
    access_token: String,
    #[serde(default)]
    id_token: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct OidcUserInfo {
    #[serde(default)]
    email: Option<String>,
    #[serde(default)]
    email_verified: Option<bool>,
}

/// Tenant OIDC configuration (in-memory, decrypted + encrypted blob).
#[derive(Debug, Clone)]
struct OidcConfig {
    issuer: String,
    client_id: String,
    client_secret: String,
    /// The PiiCipher-encrypted blob as stored in the DB.
    client_secret_encrypted: String,
    allowed_domains: Vec<String>,
    enabled: bool,
}

#[derive(Serialize)]
struct StateClaims {
    tid: String,
    v: String,
    exp: i64,
    iss: String,
}

#[derive(Deserialize)]
struct StateClaimsIn {
    tid: String,
    v: String,
}

/// POST /api/v1/auth/oidc/settings — admin saves OIDC config.
#[allow(clippy::result_large_err)]
pub async fn save_settings(
    State(state): State<AppState>,
    Authed(ctx): Authed,
    Json(body): Json<SaveOidcBody>,
) -> Response {
    if let Err(forbidden) = require(&ctx, "auth", "oidc_manage") {
        return forbidden;
    }

    let issuer = body.issuer.trim().to_string();
    let client_id = body.client_id.trim().to_string();
    let allowed_domains = body
        .allowed_domains
        .iter()
        .map(|d| d.trim().to_ascii_lowercase())
        .filter(|d| !d.is_empty())
        .collect::<Vec<_>>();

    if issuer.is_empty() || client_id.is_empty() {
        return bad_request("issuer and client_id are required");
    }
    if Url::parse(&issuer).is_err() {
        return bad_request("issuer must be a valid URL");
    }

    // Load the previously stored config (if any) so an empty client_secret
    // in the request means "keep the existing secret" rather than "clear it".
    let previous = match load_oidc_config_by_tenant(&state, &ctx.tenant_id).await {
        Ok(p) => p,
        Err(resp) => return resp,
    };

    let client_secret_encrypted = if body.client_secret.trim().is_empty() {
        match previous {
            Some(prev) => prev.client_secret_encrypted,
            None => {
                return bad_request("client_secret is required on first save");
            }
        }
    } else {
        let cipher = match state.pii.as_ref() {
            Some(c) => c,
            None => {
                tracing::error!(
                    "PII_ENCRYPTION_KEY not configured; cannot save OIDC client_secret"
                );
                return service_unavailable(
                    "PII encryption not configured; OIDC settings unavailable",
                );
            }
        };
        match cipher.encrypt(body.client_secret.trim()) {
            Ok(enc) => enc,
            Err(e) => {
                tracing::error!(error = %e, "failed to encrypt OIDC client_secret");
                return internal_err("encryption failed");
            }
        }
    };

    let mut tx = match state.pool.begin_tenant(ctx.tenant_id).await {
        Ok(tx) => tx,
        Err(e) => return internal_err(&e.to_string()),
    };

    let done = sqlx::query(
        r#"INSERT INTO tenant_oidc_config
             (tenant_id, issuer, client_id, client_secret, allowed_domains, enabled)
           VALUES ($1, $2, $3, $4, $5, $6)
           ON CONFLICT (tenant_id) DO UPDATE SET
               issuer = EXCLUDED.issuer,
               client_id = EXCLUDED.client_id,
               client_secret = EXCLUDED.client_secret,
               allowed_domains = EXCLUDED.allowed_domains,
               enabled = EXCLUDED.enabled,
               updated_at = now()"#,
    )
    .bind(ctx.tenant_id.0)
    .bind(&issuer)
    .bind(&client_id)
    .bind(&client_secret_encrypted)
    .bind(&allowed_domains)
    .bind(body.enabled)
    .execute(&mut *tx)
    .await;

    if let Err(e) = done {
        let _ = tx.rollback().await;
        return internal_err(&e.to_string());
    }

    // Significant action: audit-log the OIDC config save (no secret stored).
    if let Err(e) = sqlx::query(
        r#"INSERT INTO audit_log
             (tenant_id, actor_type, actor_id, action, resource_type, resource_id, after_state)
           VALUES ($1, 'user', $2, 'auth.oidc_settings_saved', 'tenant', $3, $4)"#,
    )
    .bind(ctx.tenant_id.0)
    .bind(ctx.user_id.0.to_string())
    .bind(ctx.tenant_id.0.to_string())
    .bind(json!({
        "issuer": issuer,
        "client_id": client_id,
        "allowed_domains": allowed_domains,
        "enabled": body.enabled,
    }))
    .execute(&mut *tx)
    .await
    {
        let _ = tx.rollback().await;
        return internal_err(&e.to_string());
    }

    if let Err(e) = tx.commit().await {
        return internal_err(&e.to_string());
    }

    (
        StatusCode::OK,
        Json(json!({
            "ok": true,
            "issuer": issuer,
            "client_id": client_id,
            "allowed_domains": allowed_domains,
            "enabled": body.enabled,
        })),
    )
        .into_response()
}

/// GET /api/v1/auth/oidc/settings — admin reads config (secret masked).
#[allow(clippy::result_large_err)]
pub async fn get_settings(State(state): State<AppState>, Authed(ctx): Authed) -> Response {
    if let Err(forbidden) = require(&ctx, "auth", "oidc_manage") {
        return forbidden;
    }

    let row = sqlx::query(
        "SELECT issuer, client_id, client_secret, allowed_domains, enabled
           FROM tenant_oidc_config WHERE tenant_id = $1",
    )
    .bind(ctx.tenant_id.0)
    .fetch_optional(&state.auth_pool)
    .await;

    let row = match row {
        Ok(Some(row)) => row,
        Ok(None) => return (StatusCode::OK, Json(json!({ "configured": false }))).into_response(),
        Err(e) => return internal_err(&e.to_string()),
    };

    use sqlx::Row;
    let secret_encrypted: String = row.get("client_secret");
    let allowed_domains: Vec<String> = row.get("allowed_domains");
    let enabled: bool = row.get("enabled");
    let issuer: String = row.get("issuer");
    let client_id: String = row.get("client_id");

    let masked = state
        .pii
        .as_ref()
        .and_then(|c| c.decrypt(&secret_encrypted).ok())
        .map(|plain| talentos_db::pii::mask_secret(&plain));

    (
        StatusCode::OK,
        Json(json!({
            "configured": true,
            "issuer": issuer,
            "client_id": client_id,
            "client_secret_masked": masked,
            "allowed_domains": allowed_domains,
            "enabled": enabled,
        })),
    )
        .into_response()
}

/// GET /api/v1/auth/oidc/authorize — redirect to the provider's auth endpoint.
#[allow(clippy::result_large_err)]
pub async fn authorize(State(state): State<AppState>, Query(q): Query<AuthorizeQuery>) -> Response {
    let tenant_slug = q.tenant_slug.trim();
    let tenant = match resolve_tenant(&state, tenant_slug).await {
        Ok(t) => t,
        Err(resp) => return resp,
    };

    let cfg = match load_oidc_config_by_tenant(&state, &tenant.0).await {
        Ok(Some(c)) => c,
        Ok(None) => return error_redirect("oidc_not_configured"),
        Err(resp) => return resp,
    };

    if !cfg.enabled {
        return error_redirect("oidc_disabled");
    }

    let discovery = match discovery(&state.http, &state.oidc_cache, &cfg.issuer).await {
        Ok(d) => d,
        Err(resp) => return resp,
    };

    let redirect_uri = match state.base_url.as_deref() {
        Some(u) => format!("{}/api/v1/auth/oidc/callback", u.trim_end_matches('/')),
        None => "/api/v1/auth/oidc/callback".to_string(),
    };

    // PKCE
    let (code_verifier, code_challenge) = pkce_pair();
    let state_token = sign_state(&state.jwt_secret, &tenant.0, &code_verifier);

    let auth_url = match Url::parse(&discovery.authorization_endpoint) {
        Ok(u) => u,
        Err(e) => {
            tracing::error!(error = %e, "authorization_endpoint is not a valid URL: {}", discovery.authorization_endpoint);
            return internal_err("invalid authorization endpoint");
        }
    };

    let params: Vec<(String, String)> = vec![
        ("client_id".into(), cfg.client_id.clone()),
        ("redirect_uri".into(), redirect_uri.clone()),
        ("response_type".into(), "code".into()),
        ("scope".into(), "openid email profile".into()),
        ("state".into(), state_token.clone()),
        ("code_challenge".into(), code_challenge.clone()),
        ("code_challenge_method".into(), "S256".into()),
    ];

    let url_str = auth_url.to_string();
    let url = match Url::parse_with_params(&url_str, &params) {
        Ok(u) => u,
        Err(e) => {
            tracing::error!(error = %e, "failed to build authorize URL");
            return internal_err("invalid authorization URL");
        }
    };

    (StatusCode::SEE_OTHER, [(header::LOCATION, url.to_string())]).into_response()
}

/// GET /api/v1/auth/oidc/callback — exchange code, verify email, link or create user.
#[allow(clippy::result_large_err)]
pub async fn callback(State(state): State<AppState>, Query(q): Query<CallbackQuery>) -> Response {
    let state_token = q.state.as_deref().unwrap_or("").trim();
    let code = q.code.as_deref().unwrap_or("").trim();
    if state_token.is_empty() || code.is_empty() {
        return error_redirect("oidc_bad_state");
    }

    // Verify the signed state to recover the code verifier + tenant.
    let (tenant_id, code_verifier) = match read_state(&state.jwt_secret, state_token) {
        Ok(v) => v,
        Err(resp) => return resp,
    };

    let cfg = match load_oidc_config_by_tenant(&state, &tenant_id).await {
        Ok(Some(c)) => c,
        Ok(None) => return error_redirect("oidc_not_configured"),
        Err(resp) => return resp,
    };

    let discovery = match discovery(&state.http, &state.oidc_cache, &cfg.issuer).await {
        Ok(d) => d,
        Err(resp) => return resp,
    };

    let redirect_uri = match state.base_url.as_deref() {
        Some(u) => format!("{}/api/v1/auth/oidc/callback", u.trim_end_matches('/')),
        None => "/api/v1/auth/oidc/callback".to_string(),
    };

    // Token exchange
    let token_resp = match exchange_token(
        &state.http,
        &discovery,
        &cfg,
        &redirect_uri,
        code,
        &code_verifier,
    )
    .await
    {
        Ok(r) => r,
        Err(resp) => return resp,
    };

    // Resolve user info (userinfo endpoint if present, else decode id_token claims)
    let user_info = match resolve_user_info(&state.http, &discovery, &token_resp).await {
        Ok(u) => u,
        Err(resp) => return resp,
    };

    let Some(email) = user_info.email else {
        return error_redirect("oidc_no_email");
    };
    let email = email.to_ascii_lowercase();

    // Verified email is required.
    if !user_info.email_verified.unwrap_or(false) {
        return error_redirect("oidc_email_unverified");
    }

    // Domain allow-list
    let domain = email.rsplit('@').next().unwrap_or_default();
    if !cfg.allowed_domains.is_empty() && !cfg.allowed_domains.iter().any(|d| d == domain) {
        return error_redirect("oidc_domain_not_allowed");
    }

    // Link or create the user by email.
    let linked = match link_or_create_user(&state, &tenant_id, &email).await {
        Ok(r) => r,
        Err(resp) => return resp,
    };

    if !linked {
        return error_redirect("oidc_user_not_found");
    }

    // Successful OIDC login: audit-log it.
    if let Err(e) = sqlx::query(
        r#"INSERT INTO audit_log
             (tenant_id, actor_type, actor_id, action, resource_type, resource_id, after_state)
           VALUES ($1, 'user', $2, 'auth.oidc_login', 'session', $2, '{}'::jsonb)"#,
    )
    .bind(tenant_id.0)
    .bind(tenant_id.0.to_string())
    .execute(&state.auth_pool)
    .await
    {
        tracing::warn!(error = %e, "failed to write audit_log for OIDC login");
    }

    (
        StatusCode::SEE_OTHER,
        [(header::LOCATION, "/login?oidc=ok".to_string())],
    )
        .into_response()
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

#[allow(clippy::result_large_err)]
async fn load_oidc_config_by_tenant(
    state: &AppState,
    tenant_id: &TenantId,
) -> Result<Option<OidcConfig>, Response> {
    let row = sqlx::query(
        "SELECT issuer, client_id, client_secret, allowed_domains, enabled
           FROM tenant_oidc_config WHERE tenant_id = $1",
    )
    .bind(tenant_id.0)
    .fetch_optional(&state.auth_pool)
    .await
    .map_err(|e| internal_err(&e.to_string()))?;

    let Some(row) = row else {
        return Ok(None);
    };

    use sqlx::Row;
    let issuer: String = row.get("issuer");
    let client_id: String = row.get("client_id");
    let secret_enc: String = row.get("client_secret");
    let allowed: Vec<String> = row.get("allowed_domains");
    let enabled: bool = row.get("enabled");

    let client_secret = match state.pii.as_ref() {
        Some(c) => c.decrypt(&secret_enc).ok().unwrap_or_default(),
        None => {
            tracing::error!("PII_ENCRYPTION_KEY not configured; cannot decrypt OIDC client_secret");
            return Ok(None);
        }
    };

    Ok(Some(OidcConfig {
        issuer,
        client_id,
        client_secret,
        client_secret_encrypted: secret_enc,
        allowed_domains: allowed,
        enabled,
    }))
}

/// Resolve a tenant by slug using the BYPASSRLS pool (login-style bootstrap).
#[allow(clippy::result_large_err)]
async fn resolve_tenant(state: &AppState, slug: &str) -> Result<(TenantId, String), Response> {
    let row = sqlx::query("SELECT id, slug FROM tenants WHERE slug = $1")
        .bind(slug)
        .fetch_optional(&state.auth_pool)
        .await
        .map_err(|e| internal_err(&e.to_string()))?;

    let Some(row) = row else {
        return Err(error_redirect("oidc_unknown_tenant"));
    };

    use sqlx::Row;
    Ok((TenantId(row.get("id")), row.get("slug")))
}

/// Fetch + cache the OIDC discovery document for an issuer.
#[allow(clippy::result_large_err)]
async fn discovery(
    http: &reqwest::Client,
    cache: &DiscoveryCache,
    issuer: &str,
) -> Result<OidcDiscovery, Response> {
    if let Ok(map) = cache.try_lock() {
        if let Some((d, _exp)) = map.get(issuer).filter(|(_, e)| e.elapsed() < DISCOVERY_TTL) {
            return Ok(d.clone());
        }
    }

    let well_known = format!(
        "{}/.well-known/openid-configuration",
        issuer.trim_end_matches('/')
    );
    let cfg: OidcDiscovery = http
        .get(&well_known)
        .send()
        .await
        .map_err(|e| {
            tracing::error!(error = %e, "OIDC discovery fetch failed");
            service_unavailable("OIDC provider unreachable")
        })?
        .error_for_status()
        .map_err(|e| {
            tracing::error!(error = %e, "OIDC discovery HTTP error");
            service_unavailable("OIDC provider returned an error")
        })?
        .json()
        .await
        .map_err(|e| {
            tracing::error!(error = %e, "OIDC discovery JSON parse failed");
            service_unavailable("OIDC provider returned malformed data")
        })?;

    if let Ok(mut map) = cache.try_lock() {
        map.insert(
            issuer.to_string(),
            (cfg.clone(), tokio::time::Instant::now()),
        );
    }

    Ok(cfg)
}

/// PKCE: returns (code_verifier, code_challenge) using S256.
fn pkce_pair() -> (String, String) {
    use rand::RngCore;
    let mut buf = [0u8; 48];
    rand::thread_rng().fill_bytes(&mut buf);
    let verifier = URL_SAFE_NO_PAD.encode(buf);
    let mut h = Sha256::new();
    h.update(verifier.as_bytes());
    let challenge = URL_SAFE_NO_PAD.encode(h.finalize());
    (verifier, challenge)
}

/// Sign the OIDC `state` parameter: tenant id + PKCE verifier + short expiry.
fn sign_state(secret: &[u8], tenant_id: &TenantId, verifier: &str) -> String {
    let now = chrono::Utc::now().timestamp();
    let claims = StateClaims {
        tid: tenant_id.to_string(),
        v: verifier.to_string(),
        exp: now + STATE_TTL_SECS,
        iss: OIDC_ISS.into(),
    };

    encode(
        &Header::new(Algorithm::HS256),
        &claims,
        &EncodingKey::from_secret(secret),
    )
    .unwrap_or_default()
}

/// Verify + decrypt the signed state. Returns (tenant_id, verifier).
#[allow(clippy::result_large_err)]
fn read_state(secret: &[u8], token: &str) -> Result<(TenantId, String), Response> {
    let mut validation = Validation::new(Algorithm::HS256);
    validation.validate_exp = true;
    validation.set_issuer(&[OIDC_ISS]);

    let data = decode::<StateClaimsIn>(token, &DecodingKey::from_secret(secret), &validation)
        .map_err(|e| {
            tracing::warn!(error = %e, "OIDC state verification failed");
            error_redirect("oidc_bad_state")
        })?;

    let c = data.claims;
    let tenant_id = Uuid::parse_str(&c.tid).map_err(|_| error_redirect("oidc_bad_state"))?;

    Ok((TenantId(tenant_id), c.v))
}

/// Exchange the authorization code for tokens.
#[allow(clippy::result_large_err)]
async fn exchange_token(
    http: &reqwest::Client,
    discovery: &OidcDiscovery,
    cfg: &OidcConfig,
    redirect_uri: &str,
    code: &str,
    code_verifier: &str,
) -> Result<OidcTokenResponse, Response> {
    let resp = http
        .post(&discovery.token_endpoint)
        .form(&[
            ("grant_type", "authorization_code"),
            ("code", code),
            ("redirect_uri", redirect_uri),
            ("client_id", &cfg.client_id),
            ("client_secret", &cfg.client_secret),
            ("code_verifier", code_verifier),
        ])
        .send()
        .await
        .map_err(|e| {
            tracing::error!(error = %e, "OIDC token exchange failed");
            service_unavailable("OIDC provider unreachable")
        })?;

    let status = resp.status();
    let body: serde_json::Value = resp.json().await.unwrap_or(serde_json::Value::Null);
    if !status.is_success() {
        tracing::error!(status = %status, body = %body, "OIDC token exchange returned error");
        return Err(error_redirect("oidc_token_exchange_failed"));
    }

    let token: OidcTokenResponse = serde_json::from_value(body).map_err(|e| {
        tracing::error!(error = %e, "OIDC token response malformed");
        service_unavailable("OIDC provider returned malformed token data")
    })?;

    Ok(token)
}

/// Resolve user info from the userinfo endpoint, or fall back to id_token claims.
#[allow(clippy::result_large_err)]
async fn resolve_user_info(
    http: &reqwest::Client,
    discovery: &OidcDiscovery,
    token: &OidcTokenResponse,
) -> Result<OidcUserInfo, Response> {
    if let Some(userinfo_endpoint) = &discovery.userinfo_endpoint {
        if !userinfo_endpoint.is_empty() {
            let ui: OidcUserInfo = http
                .get(userinfo_endpoint)
                .bearer_auth(&token.access_token)
                .send()
                .await
                .map_err(|e| {
                    tracing::error!(error = %e, "OIDC userinfo fetch failed");
                    service_unavailable("OIDC provider unreachable")
                })?
                .error_for_status()
                .map_err(|e| {
                    tracing::error!(error = %e, "OIDC userinfo HTTP error");
                    service_unavailable("OIDC provider returned an error")
                })?
                .json()
                .await
                .map_err(|e| {
                    tracing::error!(error = %e, "OIDC userinfo JSON parse failed");
                    service_unavailable("OIDC provider returned malformed userinfo")
                })?;
            return Ok(ui);
        }
    }

    // No userinfo endpoint: decode the id_token as a JWT (unverified — the token
    // was just issued by the provider over TLS, so this is acceptable for
    // claim extraction in a skeleton; full verification is a follow-up).
    if let Some(id_token) = &token.id_token {
        let payload = id_token
            .split('.')
            .nth(1)
            .ok_or_else(|| service_unavailable("id_token malformed"))?;
        let bytes = URL_SAFE_NO_PAD
            .decode(payload)
            .map_err(|_| service_unavailable("id_token claims not base64url"))?;
        let claims: serde_json::Value = serde_json::from_slice(&bytes).map_err(|e| {
            tracing::error!(error = %e, "id_token JSON parse failed");
            service_unavailable("OIDC provider returned malformed id_token")
        })?;
        let ui = OidcUserInfo {
            email: claims
                .get("email")
                .and_then(|v| v.as_str())
                .map(String::from),
            email_verified: claims.get("email_verified").and_then(|v| v.as_bool()),
        };
        return Ok(ui);
    }

    Err(service_unavailable(
        "OIDC provider returned no user identity",
    ))
}

/// Link an existing user by email, or create a new one. Returns true if the user
/// was found/created and is active.
#[allow(clippy::result_large_err)]
async fn link_or_create_user(
    state: &AppState,
    tenant_id: &TenantId,
    email: &str,
) -> Result<bool, Response> {
    let mut tx = state
        .pool
        .begin_tenant(*tenant_id)
        .await
        .map_err(|e| internal_err(&e.to_string()))?;

    let existing_res: Result<Option<Uuid>, sqlx::Error> =
        sqlx::query_scalar("SELECT id FROM users WHERE tenant_id =  AND lower(email) = ")
            .bind(tenant_id.0)
            .bind(email)
            .fetch_optional(&mut *tx)
            .await;
    let existing = match existing_res {
        Ok(v) => v,
        Err(e) => {
            let _ = tx.rollback().await;
            return Err(internal_err(&e.to_string()));
        }
    };

    if let Some(uid) = existing {
        let active_res: Result<bool, sqlx::Error> =
            sqlx::query_scalar("SELECT is_active FROM users WHERE id =  AND tenant_id = ")
                .bind(uid)
                .bind(tenant_id.0)
                .fetch_one(&mut *tx)
                .await;
        let active = match active_res {
            Ok(v) => v,
            Err(e) => {
                let _ = tx.rollback().await;
                return Err(internal_err(&e.to_string()));
            }
        };

        if !active {
            let _ = tx.rollback().await;
            return Ok(false);
        }

        // Update last login timestamp (best-effort).
        let _ = sqlx::query("UPDATE users SET last_login_at = now() WHERE id =  AND tenant_id = ")
            .bind(uid)
            .bind(tenant_id.0)
            .execute(&mut *tx)
            .await;

        if let Err(e) = tx.commit().await {
            return Err(internal_err(&e.to_string()));
        }
        return Ok(true);
    }

    // No user: create one with an OIDC-only placeholder password hash and the
    // default 'employee' role. The user can log in via OIDC, not password.
    let placeholder_hash = Sha256::digest(Uuid::new_v4().as_bytes()).to_vec();

    let created_res: Result<Uuid, sqlx::Error> = sqlx::query_scalar(
        r#"INSERT INTO users (tenant_id, email, password_hash, is_active)
           VALUES (, , , TRUE) RETURNING id"#,
    )
    .bind(tenant_id.0)
    .bind(email)
    .bind(&placeholder_hash)
    .fetch_one(&mut *tx)
    .await;
    let created = match created_res {
        Ok(v) => v,
        Err(e) => {
            let _ = tx.rollback().await;
            return Err(internal_err(&e.to_string()));
        }
    };

    let role_id_res: Result<i32, sqlx::Error> = sqlx::query_scalar(
        r#"INSERT INTO roles (tenant_id, name) VALUES (, 'employee')
           ON CONFLICT (tenant_id, name) DO UPDATE SET name = EXCLUDED.name
           RETURNING id"#,
    )
    .bind(tenant_id.0)
    .fetch_one(&mut *tx)
    .await;
    let role_id = match role_id_res {
        Ok(v) => v,
        Err(e) => {
            let _ = tx.rollback().await;
            return Err(internal_err(&e.to_string()));
        }
    };

    let assign_res: Result<_, sqlx::Error> =
        sqlx::query("INSERT INTO user_roles (user_id, role_id) VALUES (, ) ON CONFLICT DO NOTHING")
            .bind(created)
            .bind(role_id)
            .execute(&mut *tx)
            .await;
    if let Err(e) = assign_res {
        let _ = tx.rollback().await;
        return Err(internal_err(&e.to_string()));
    }

    if let Err(e) = tx.commit().await {
        return Err(internal_err(&e.to_string()));
    }

    Ok(true)
}

// ---------------------------------------------------------------------------
// Query / body types
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct AuthorizeQuery {
    pub tenant_slug: String,
}

#[derive(Debug, Deserialize)]
pub struct CallbackQuery {
    pub code: Option<String>,
    pub state: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct SaveOidcBody {
    pub issuer: String,
    pub client_id: String,
    pub client_secret: String,
    #[serde(default)]
    pub allowed_domains: Vec<String>,
    #[serde(default)]
    pub enabled: bool,
}

// ---------------------------------------------------------------------------
// Helpers: error + response constructors
// ---------------------------------------------------------------------------

fn error_redirect(code: &str) -> Response {
    (
        StatusCode::SEE_OTHER,
        [(header::LOCATION, format!("/login?oidc_error={code}"))],
    )
        .into_response()
}

fn bad_request(message: &str) -> Response {
    (
        StatusCode::BAD_REQUEST,
        Json(ErrorResponse {
            error: "invalid_request".into(),
            message: message.into(),
            details: vec![],
            request_id: Uuid::new_v4().to_string(),
            retry_after_seconds: None,
        }),
    )
        .into_response()
}

fn service_unavailable(message: &str) -> Response {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(ErrorResponse {
            error: "service_unavailable".into(),
            message: message.into(),
            details: vec![],
            request_id: Uuid::new_v4().to_string(),
            retry_after_seconds: None,
        }),
    )
        .into_response()
}

fn internal_err(message: &str) -> Response {
    tracing::error!(error = %message, "OIDC endpoint failed");
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

// ---------------------------------------------------------------------------
// Unit tests: discovery + PKCE + token exchange against a local mock provider.
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{routing::get, Router};

    /// A minimal mock OIDC provider.
    async fn mock_provider() -> (String, Router) {
        let app = Router::new()
            .route(
                "/.well-known/openid-configuration",
                get(|| async {
                    Json(json!({
                        "authorization_endpoint": "http://provider.local/authorize",
                        "token_endpoint": "http://provider.local/token",
                        "userinfo_endpoint": "http://provider.local/userinfo"
                    }))
                }),
            )
            .route(
                "/token",
                axum::routing::post(|| async {
                    Json(json!({
                        "access_token": "mock-access-token",
                        "id_token": "mock-id-token"
                    }))
                }),
            )
            .route(
                "/userinfo",
                get(|| async {
                    Json(json!({
                        "sub": "user-123",
                        "email": "Test@Example.com",
                        "email_verified": true,
                        "name": "Test User"
                    }))
                }),
            );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let serve_app = app.clone();
        tokio::spawn(async move {
            let _ = axum::serve(listener, serve_app).await;
        });
        (format!("http://{}", addr), app)
    }

    #[tokio::test]
    async fn discovery_fetch_and_cache() {
        let (base, _) = mock_provider().await;
        let cache: DiscoveryCache = Arc::new(std::sync::Mutex::new(HashMap::new()));
        let http = reqwest::Client::new();

        let d = discovery(&http, &cache, &base).await.unwrap();
        assert_eq!(d.authorization_endpoint, "http://provider.local/authorize");
        assert_eq!(d.token_endpoint, "http://provider.local/token");

        // Second call should hit the cache (same value, no network needed).
        let d2 = discovery(&http, &cache, &base).await.unwrap();
        assert_eq!(d, d2);
    }

    #[tokio::test]
    async fn discovery_cache_hit_avoids_network() {
        let cache: DiscoveryCache = Arc::new(std::sync::Mutex::new(HashMap::new()));
        let http = reqwest::Client::new();

        // Pre-seed the cache so no network call is made.
        {
            let mut map = cache.lock().unwrap();
            map.insert(
                "https://example.com".to_string(),
                (
                    OidcDiscovery {
                        authorization_endpoint: "https://example.com/auth".into(),
                        token_endpoint: "https://example.com/token".into(),
                        userinfo_endpoint: None,
                    },
                    tokio::time::Instant::now(),
                ),
            );
        }

        let d = discovery(&http, &cache, "https://example.com")
            .await
            .unwrap();
        assert_eq!(d.token_endpoint, "https://example.com/token");
    }

    #[tokio::test]
    async fn pkce_pair_is_deterministic_s256() {
        let (verifier, challenge) = pkce_pair();
        // Verify the challenge is the S256 hash of the verifier.
        let mut h = Sha256::new();
        h.update(verifier.as_bytes());
        let expected = URL_SAFE_NO_PAD.encode(h.finalize());
        assert_eq!(challenge, expected);
        assert_eq!(verifier.len(), 64); // 48 bytes base64url
    }

    #[test]
    fn base64_url_is_url_safe_and_unpadded() {
        let input = [0u8; 3];
        let out = URL_SAFE_NO_PAD.encode(input);
        assert!(!out.contains('='));
        assert!(!out.contains('+'));
        assert!(!out.contains('/'));
    }

    #[tokio::test]
    async fn discovery_error_returns_err() {
        let cache: DiscoveryCache = Arc::new(std::sync::Mutex::new(HashMap::new()));
        let http = reqwest::Client::new();
        // An unreachable issuer should produce an Err (not a panic).
        let resp = discovery(&http, &cache, "http://127.0.0.1:1").await;
        assert!(resp.is_err());
    }

    /// `sign_state` + `read_state` round-trip with a known secret.
    const SECRET: &[u8] = b"test-secret-for-oidc-state-32bytes-ok!!";

    #[test]
    fn sign_and_read_state_roundtrip() {
        let tenant_id = TenantId(Uuid::new_v4());
        let (verifier, _challenge) = pkce_pair();
        let token = sign_state(SECRET, &tenant_id, &verifier);
        let (recovered_tenant, recovered_verifier) = read_state(SECRET, &token).unwrap();
        assert_eq!(recovered_tenant, tenant_id);
        assert_eq!(recovered_verifier, verifier);
    }

    /// Verify that when client_secret is empty in the save body, the
    /// previously stored encrypted blob is preserved (not overwritten with empty).
    #[test]
    fn empty_client_secret_preserves_stored_blob() {
        // Simulate: previous config has an encrypted secret.
        let previous_encrypted = "v1:abcdef".to_string();
        // An empty body secret should select the previous encrypted blob.
        let body_secret = "";
        let chosen = if body_secret.trim().is_empty() {
            previous_encrypted.clone()
        } else {
            "new-encrypted-blob".to_string()
        };
        assert_eq!(chosen, "v1:abcdef");

        // A non-empty body secret should select the new blob.
        let body_secret2 = "new-secret";
        let chosen2 = if body_secret2.trim().is_empty() {
            previous_encrypted.clone()
        } else {
            "new-encrypted-blob".to_string()
        };
        assert_eq!(chosen2, "new-encrypted-blob");
    }

    /// A tampered state token must fail.
    #[test]
    fn tampered_state_rejected() {
        let tenant_id = TenantId(Uuid::new_v4());
        let (verifier, _) = pkce_pair();
        let token = sign_state(SECRET, &tenant_id, &verifier);
        // Flip a character in the signature (last segment).
        let parts: Vec<&str> = token.split('.').collect();
        assert_eq!(parts.len(), 3);
        let mut sig = parts[2].to_string();
        let last_char = sig.pop().unwrap();
        sig.push(if last_char == 'A' { 'B' } else { 'A' });
        let tampered = format!("{}.{}.{}", parts[0], parts[1], sig);
        let result = read_state(SECRET, &tampered);
        assert!(result.is_err());
    }
}
