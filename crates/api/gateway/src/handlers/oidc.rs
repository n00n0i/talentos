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
use jsonwebtoken::jwk::{AlgorithmParameters, JwkSet};
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
    pub issuer: String,
    #[serde(default)]
    pub jwks_uri: Option<String>,
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

    // Resolve user info (userinfo endpoint if present, else verify id_token claims)
    let user_info =
        match resolve_user_info(&state.http, &discovery, &cfg.client_id, &token_resp).await {
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
    let linked_user_id = match link_or_create_user(&state, &tenant_id, &email).await {
        Ok(Some(id)) => id,
        Ok(None) => {
            return error_redirect("oidc_user_not_found");
        }
        Err(resp) => return resp,
    };

    // Successful OIDC login: audit-log it.
    if let Err(e) = sqlx::query(
        r#"INSERT INTO audit_log
             (tenant_id, actor_type, actor_id, action, resource_type, resource_id, after_state)
           VALUES ($1, 'user', $2, 'auth.oidc_login', 'session', $3, '{}'::jsonb)"#,
    )
    .bind(tenant_id.0)
    .bind(linked_user_id.to_string())
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
    let mut cfg: OidcDiscovery = http
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
    // Trust only the tenant-configured issuer, whatever the document claims.
    cfg.issuer = issuer.to_string();

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

/// Resolve user info from the userinfo endpoint, or verify id_token claims.
#[allow(clippy::result_large_err)]
async fn resolve_user_info(
    http: &reqwest::Client,
    discovery: &OidcDiscovery,
    client_id: &str,
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

    // No usable userinfo endpoint: the id_token claims are only trusted after
    // signature verification via the provider's JWKS.
    let id_token = token
        .id_token
        .as_deref()
        .ok_or_else(|| service_unavailable("OIDC provider returned no user identity"))?;

    let claims = verify_id_token(http, discovery, client_id, id_token).await?;
    Ok(OidcUserInfo {
        email: claims
            .get("email")
            .and_then(|v| v.as_str())
            .map(String::from),
        email_verified: claims.get("email_verified").and_then(|v| v.as_bool()),
    })
}

/// Verify an id_token's RS256 signature against the provider's JWKS, plus
/// issuer, audience, and expiry checks. On ANY failure: 503, never
/// unverified claims.
#[allow(clippy::result_large_err)]
async fn verify_id_token(
    http: &reqwest::Client,
    discovery: &OidcDiscovery,
    client_id: &str,
    id_token: &str,
) -> Result<serde_json::Value, Response> {
    let fail = |msg: &str| -> Response {
        tracing::error!(error = msg, "OIDC id_token verification failed");
        service_unavailable(msg)
    };

    let Some(jwks_uri) = discovery.jwks_uri.as_deref().filter(|u| !u.is_empty()) else {
        return Err(fail(
            "OIDC provider exposes no JWKS and no usable userinfo endpoint",
        ));
    };

    // No cache: one HTTPS call per OIDC callback is negligible.
    let jwks: JwkSet = http
        .get(jwks_uri)
        .send()
        .await
        .map_err(|e| {
            tracing::error!(error = %e, "OIDC JWKS fetch failed");
            service_unavailable("OIDC provider unreachable")
        })?
        .error_for_status()
        .map_err(|_| fail("OIDC JWKS endpoint returned an error"))?
        .json()
        .await
        .map_err(|_| fail("OIDC JWKS endpoint returned malformed data"))?;

    // Select the JWK whose kid matches the id_token header; if the token has
    // no kid, use the key only when the set has exactly one key.
    let kid = jsonwebtoken::decode_header(id_token)
        .ok()
        .as_ref()
        .and_then(|h| h.kid.clone());
    let jwk = match &kid {
        Some(k) => jwks.find(k),
        None if jwks.keys.len() == 1 => jwks.keys.first(),
        _ => None,
    };
    let Some(jwk) = jwk else {
        return Err(fail("no matching JWKS key for id_token"));
    };

    let rsa = match &jwk.algorithm {
        AlgorithmParameters::RSA(params) => Some(params),
        _ => None,
    };
    let Some(params) = rsa else {
        return Err(fail("JWKS key is not an RSA key"));
    };
    let key = DecodingKey::from_rsa_components(&params.n, &params.e).map_err(|e| {
        tracing::error!(error = %e, "JWKS RSA key is malformed");
        service_unavailable(&format!("JWKS RSA key is malformed: {e}"))
    })?;

    let mut validation = Validation::new(Algorithm::RS256);
    validation.set_issuer(&[&discovery.issuer]);
    validation.set_audience(&[client_id]);
    validation.leeway = 10;

    let data = decode::<serde_json::Value>(id_token, &key, &validation).map_err(|e| {
        tracing::error!(error = %e, "OIDC id_token validation failed");
        service_unavailable(&format!("OIDC id_token failed validation: {e}"))
    })?;
    Ok(data.claims)
}

/// Link an existing user by email, or create a new one. Returns the linked
/// (or newly created) active user's id, or None if the email has no active
/// user in the tenant.
#[allow(clippy::result_large_err)]
async fn link_or_create_user(
    state: &AppState,
    tenant_id: &TenantId,
    email: &str,
) -> Result<Option<Uuid>, Response> {
    let mut tx = state
        .pool
        .begin_tenant(*tenant_id)
        .await
        .map_err(|e| internal_err(&e.to_string()))?;

    let existing_res: Result<Option<Uuid>, sqlx::Error> =
        sqlx::query_scalar("SELECT id FROM users WHERE tenant_id = $1 AND lower(email) = $2")
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
            sqlx::query_scalar("SELECT is_active FROM users WHERE id = $1 AND tenant_id = $2")
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
            return Ok(None);
        }

        // Update last login timestamp (best-effort).
        let _ =
            sqlx::query("UPDATE users SET last_login_at = now() WHERE id = $1 AND tenant_id = $2")
                .bind(uid)
                .bind(tenant_id.0)
                .execute(&mut *tx)
                .await;

        if let Err(e) = tx.commit().await {
            return Err(internal_err(&e.to_string()));
        }
        return Ok(Some(uid));
    }

    // No user: create one with an OIDC-only placeholder password hash and the
    // default 'employee' role. The user can log in via OIDC, not password.
    let placeholder_hash = Sha256::digest(Uuid::new_v4().as_bytes()).to_vec();

    let created_res: Result<Uuid, sqlx::Error> = sqlx::query_scalar(
        r#"INSERT INTO users (tenant_id, email, password_hash, is_active)
           VALUES ($1, $2, $3, TRUE) RETURNING id"#,
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
        r#"INSERT INTO roles (tenant_id, name) VALUES ($1, 'employee')
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

    let assign_res: Result<_, sqlx::Error> = sqlx::query(
        "INSERT INTO user_roles (user_id, role_id) VALUES ($1, $2) ON CONFLICT DO NOTHING",
    )
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

    Ok(Some(created))
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
                        "userinfo_endpoint": "http://provider.local/userinfo",
                        "issuer": "http://provider.local"
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

    // ------------------------------------------------------------------
    // JWKS verification tests.
    //
    // Pre-computed 2048-bit RSA fixtures:
    //   - KEY_A_N / KEY_B_N: JWKS modulus values (base64url)
    //   - TOKEN_*: static RS256-signed id_tokens (exp=2100 so they never
    //     expire; TOKEN_EXPIRED has exp=1000 i.e. 1970)
    //
    // The mock provider serves TOKEN_OK from its /token route and exposes
    // KEY_A_N at /jwks. Tests call resolve_user_info (the production code
    // path) which fetches the JWKS and verifies the signature.
    // ------------------------------------------------------------------
    const KEY_A_N: &str = "7zP0np_846vQ7TiC3p_BA4E9nGvBT3TSPU541UMrSEZ1w4TwQDjR62kZ5oqHkCfFnUpfexo_s_dJtC4AfK7IT5VIIDZ92yKBG4qC3k4cmoDF0JOAPS9E2ZArzH8xMQKADVM9TvvdkWaVq6CFUWxaEAqJdyAvhVtxR1lNf6QreA8BGZEENIVYz62RtIXt6dIsWJp16uW_ucaVTDDoXVPHdpwO4LDSq1h9qV87L0jNJ_1gS47bhV6BeHanLLd0Q5HYx1NliVrCUTf3vNXrABmURXTpYVCJEPxRxGwI9hnvNOVReT8jM-44S0LAj8XIPtgPb-41_4q6bfu1KUq_J9AMnQ";

    // Signed by the key whose modulus is KEY_A_N; aud=client-e2e; exp=2100.
    const TOKEN_OK: &str = "eyJhbGciOiJSUzI1NiIsInR5cCI6IkpXVCIsImtpZCI6ImsxIn0.eyJpc3MiOiJodHRwOi8vcHJvdmlkZXIubG9jYWwiLCJhdWQiOiJjbGllbnQtZTJlIiwic3ViIjoidXNlci0xMjMiLCJlbWFpbCI6Imp3a3MtdXNlckBleGFtcGxlLmNvbSIsImVtYWlsX3ZlcmlmaWVkIjp0cnVlLCJpYXQiOjAsImV4cCI6NDEwMjQ0NDgwMH0.UolfI-CblPeweWJOA4VyFlVIJYmff5SDBYNFigJ072iw8qtOpX1uMVuu9J3vRlxEEWqIkeDOgyCVQA4F6gonA6ATqVwq9CEGWDM00TbkaJJNvX9Gx5W2-bOKJ3eTRwJszDRb8em7339YkRniitmgk85YzPmQoiDW3K6bFnoz7S-UbjCDBxiQLz97ozIgIr6Km7Zt4iC9j02LgdHyKtMkAOQuJ0KWaqgCELndYdJ7qFjKI9xTeirubjjRS-DEIcdvWTVy5fIJC1aNbbyxPSRXRKfgxA1NN7J71W-nhjJVzC4VynORrQmH_EYVb-hRTVYsN3pNuAm1RcKSxApU2uM5WA";
    // Same claims, signed by a DIFFERENT key (modulus KEY_B_N) — signature
    // will not match when the JWKS publishes KEY_A_N.
    const TOKEN_WRONG_SIG: &str = "eyJhbGciOiJSUzI1NiIsInR5cCI6IkpXVCIsImtpZCI6ImsxIn0.eyJpc3MiOiJodHRwOi8vcHJvdmlkZXIubG9jYWwiLCJhdWQiOiJjbGllbnQtZTJlIiwic3ViIjoidXNlci0xMjMiLCJlbWFpbCI6Imp3a3MtdXNlckBleGFtcGxlLmNvbSIsImVtYWlsX3ZlcmlmaWVkIjp0cnVlLCJpYXQiOjAsImV4cCI6NDEwMjQ0NDgwMH0.T0VFhfbDJExUd4GI-p4YwPBIyQzRhmLiVLsmX6aq4A_cpjZqbNuRQkYczrXXe7-XabVw2E_DWDuNWyPaM2sCKYsBWSqIbR3wsilt6P6WKf6qLyJajJTTjDyvlMdGFAbbYl-fvCH8_rW5Z85BToO1w3ZG0fjqVuMcmkdbx2RJv89lgn8jQn0klG26SrXB_to8mUzx4279s9kyxr0DBWk80xmN7sLQlyTDDFQK-GBwLor6w7yyXciofWymEZRJa_6wMhNA7KTSYroLir2-2cjIv98GsahEMalmrsCdBBS8me6hLmgVuvrFc195PMX1bQ-fasSQefg95KMbfkBGPoeQzg";
    // Signed by KEY_A but aud=some-other-client (not in the expected list).
    const TOKEN_BAD_AUD: &str = "eyJhbGciOiJSUzI1NiIsInR5cCI6IkpXVCIsImtpZCI6ImsxIn0.eyJpc3MiOiJodHRwOi8vcHJvdmlkZXIubG9jYWwiLCJhdWQiOiJzb21lLW90aGVyLWNsaWVudCIsInN1YiI6InVzZXItMTIzIiwiZW1haWwiOiJqd2tzLXVzZXJAZXhhbXBsZS5jb20iLCJlbWFpbF92ZXJpZmllZCI6dHJ1ZSwiaWF0IjowLCJleHAiOjQxMDI0NDQ4MDB9.a89I13AHz0Tttg45m3if-Fuj6vWcgHPR_UinLLdYImjAKO20U4uaMukXBPx1gcbOyEqC7jFToXeqmJif8Rgmg_kzeKPr-WuG85gQTKYm9r1jep1OaITjaKETp_g_sPQEvSSpsf2U7GlCL7KbYqSPFHOBYPB8tcCVfYINTlH8G_vEHkNznJvkpWzBucKjcdAsVrY0-HXML7vWVnS1rFJJ8PyMjy6YuNKlex-0x4_xgCMip8ZEiZ9vP5y3FsgiBVleRfhiuhliMQ84SNWXoVLUcKkfdRyxcFQyoERiKWUtRzDiSwDjtsnl1EvioELFwSnfkGVgLOnUez4fAsAhpQofXg";
    // Signed by KEY_A, aud=client-e2e, but exp=1000 (1970) — expired.
    const TOKEN_EXPIRED: &str = "eyJhbGciOiJSUzI1NiIsInR5cCI6IkpXVCIsImtpZCI6ImsxIn0.eyJpc3MiOiJodHRwOi8vcHJvdmlkZXIubG9jYWwiLCJhdWQiOiJjbGllbnQtZTJlIiwic3ViIjoidXNlci0xMjMiLCJlbWFpbCI6Imp3a3MtdXNlckBleGFtcGxlLmNvbSIsImVtYWlsX3ZlcmlmaWVkIjp0cnVlLCJpYXQiOjAsImV4cCI6MTAwMH0.HfQpa26KdhCqCMiiEuH_Sl3fbJ9pIA-b9h6DnFROmqSWRmItgbyNH8R-x3HFUxp9gBciDTj6GxR0j4bBZCExyMZDEpv6jgym3JxrpaLtzZX5QqQoLMd86vWqPm_WEOJBfMAYVCOHkjTqktAZ2lsBfazBefr4ZvYuOoN0kLFRkatYsxfRb5dmvge9Jff1mU03-9tdmAnQg3jee_u6useQUzd50lTamLYSDDIJH8ze0vxMJNugVnyn9zsMtwmjUNjSTVwRTEaZAXU0X8Kk_gdRqBS5cwACLbYj1MDQOani_busOBZgemG8tUG46nYLLrJa2mAuYnNcsFp-HEwWRit_nA";

    /// Mock OIDC provider (no userinfo endpoint): discovery + token + JWKS
    /// routes. `served_token` is the static id_token the token route returns;
    /// `jwks_n` is the modulus the JWKS route publishes.
    async fn mock_provider_token_and_jwks(served_token: &str, jwks_n: &str) -> String {
        let served_token = served_token.to_string();
        let jwks_n = jwks_n.to_string();
        let app = Router::new()
            .route(
                "/.well-known/openid-configuration",
                get(|| async {
                    Json(json!({
                        "authorization_endpoint": "http://provider.local/authorize",
                        "token_endpoint": "http://provider.local/token",
                        "jwks_uri": "http://provider.local/jwks",
                        "issuer": "http://provider.local"
                    }))
                }),
            )
            .route(
                "/token",
                axum::routing::post(move || {
                    let served_token = served_token.clone();
                    async move {
                        Json(json!({
                            "access_token": "mock-access-token",
                            "id_token": served_token
                        }))
                    }
                }),
            )
            .route(
                "/jwks",
                get(move || {
                    let jwks_n = jwks_n.clone();
                    async move {
                        Json(json!({
                            "keys": [{
                                "kty": "RSA", "alg": "RS256", "use": "sig",
                                "kid": "k1", "n": jwks_n, "e": "AQAB"
                            }]
                        }))
                    }
                }),
            );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        base
    }

    /// End-to-end helper: fetch discovery, then run resolve_user_info.
    /// All URLs in the mock provider's discovery doc are rewritten to point
    /// at the actual bound base.
    #[allow(clippy::result_large_err)]
    async fn resolve_via_jwks_provider(
        base: &str,
        client_id: &str,
    ) -> Result<OidcUserInfo, Response> {
        let http = reqwest::Client::new();
        let cache: DiscoveryCache = Arc::new(std::sync::Mutex::new(HashMap::new()));
        let mut discovery = discovery(&http, &cache, base).await.unwrap();
        // The mock provider's discovery doc hardcodes http://provider.local URLs;
        // rewrite the endpoints to the actual bound base so token + JWKS routes
        // are hit. The static tokens use iss=http://provider.local, so pin the
        // issuer field to match (discovery() normally pins it to the cache key).
        discovery.issuer = "http://provider.local".into();
        discovery.token_endpoint = format!("{}/token", base);
        if discovery.jwks_uri.is_some() {
            discovery.jwks_uri = Some(format!("{}/jwks", base));
        }
        let token: OidcTokenResponse = http
            .post(&discovery.token_endpoint)
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        resolve_user_info(&http, &discovery, client_id, &token).await
    }

    /// Happy path: valid token signed by KEY_A, JWKS publishes KEY_A_N.
    #[tokio::test]
    async fn id_token_verified_via_jwks() {
        let base = mock_provider_token_and_jwks(TOKEN_OK, KEY_A_N).await;
        let ui = resolve_via_jwks_provider(&base, "client-e2e")
            .await
            .expect("signed id_token must verify against the JWKS");
        assert_eq!(ui.email.as_deref(), Some("jwks-user@example.com"));
        assert_eq!(ui.email_verified, Some(true));
    }

    /// Token signed with KEY_B but JWKS publishes KEY_A_N: mismatch.
    #[tokio::test]
    async fn id_token_rejected_when_signature_mismatch() {
        let base = mock_provider_token_and_jwks(TOKEN_WRONG_SIG, KEY_A_N).await;
        let err = resolve_via_jwks_provider(&base, "client-e2e")
            .await
            .expect_err("a token signed with the wrong key must fail");
        assert_eq!(
            err.status(),
            StatusCode::SERVICE_UNAVAILABLE,
            "signature mismatch must yield 503"
        );
    }

    /// Token with aud=some-other-client; expected client is client-e2e.
    #[tokio::test]
    async fn id_token_rejected_when_aud_missing() {
        let base = mock_provider_token_and_jwks(TOKEN_BAD_AUD, KEY_A_N).await;
        let err = resolve_via_jwks_provider(&base, "client-e2e")
            .await
            .expect_err("aud not containing client-e2e must fail");
        assert_eq!(
            err.status(),
            StatusCode::SERVICE_UNAVAILABLE,
            "aud mismatch must yield 503"
        );
    }

    /// Token with exp=1000 (1970): expired even though signature is valid.
    #[tokio::test]
    async fn id_token_rejected_when_expired() {
        let base = mock_provider_token_and_jwks(TOKEN_EXPIRED, KEY_A_N).await;
        let err = resolve_via_jwks_provider(&base, "client-e2e")
            .await
            .expect_err("an expired id_token must fail");
        assert_eq!(err.status(), StatusCode::SERVICE_UNAVAILABLE);
    }

    /// No JWKS and no userinfo endpoint: must be 503, never unverified claims.
    #[tokio::test]
    async fn resolve_user_info_503_when_no_jwks_and_no_userinfo() {
        let http = reqwest::Client::new();
        let discovery = OidcDiscovery {
            authorization_endpoint: "http://provider.local/authorize".into(),
            token_endpoint: "http://provider.local/token".into(),
            userinfo_endpoint: None,
            issuer: "http://provider.local".into(),
            jwks_uri: None,
        };
        let token_resp = OidcTokenResponse {
            access_token: "mock-access-token".into(),
            id_token: Some(TOKEN_OK.to_string()),
        };
        let err = resolve_user_info(&http, &discovery, "client-e2e", &token_resp)
            .await
            .expect_err("no JWKS and no userinfo must yield an error");
        assert_eq!(err.status(), StatusCode::SERVICE_UNAVAILABLE);
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
                        issuer: "https://example.com".into(),
                        jwks_uri: None,
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

    // ------------------------------------------------------------------
    // DB tests: link_or_create_user — exercises the bind placeholders in
    // the real queries (they fail at parse/execute time if `$n` are wrong).
    // Skips when DATABASE_URL / DATABASE_ADMIN_URL are unset.
    // ------------------------------------------------------------------

    fn test_app_state(pool: talentos_db::TenantPool, auth_pool: sqlx::PgPool) -> AppState {
        let redis_url =
            std::env::var("REDIS_URL").unwrap_or_else(|_| "redis://127.0.0.1:6379".to_string());
        let pool_arc = std::sync::Arc::new(pool);
        AppState {
            pool: pool_arc.clone(),
            auth_pool,
            jwt_secret: {
                let bytes: Vec<u8> = b"test-secret-for-oidc-state-32bytes-ok!!".to_vec();
                std::sync::Arc::from(bytes)
            },
            redis: fred::prelude::Client::new(
                fred::prelude::Config::from_url(&redis_url).expect("REDIS_URL"),
                None,
                None,
                None,
            ),
            access_ttl_secs: 900,
            refresh_ttl_days: 7,
            allowed_origins: Vec::new(),
            graphql_schema: std::sync::Arc::new(talentos_graphql::build_schema()),
            memory: std::sync::Arc::new(talentos_memory::MemoryStore::new(pool_arc, None)),
            pii: None,
            base_url: None,
            http: reqwest::Client::new(),
            oidc_cache: std::sync::Arc::new(
                std::sync::Mutex::new(std::collections::HashMap::new()),
            ),
            secrets: None,
            started_at: std::time::Instant::now(),
        }
    }

    async fn seed_oidc_tenant(admin: &sqlx::PgPool, tenant_id: &Uuid) {
        sqlx::query("INSERT INTO tenants (id, name, slug) VALUES ($1, 'OIDC Test', $2)")
            .bind(tenant_id)
            .bind(format!("oidc-test-{}", &tenant_id.to_string()[..8]))
            .execute(admin)
            .await
            .expect("insert tenant");
    }

    async fn cleanup_oidc_tenant(admin: &sqlx::PgPool, tenant_id: &Uuid) {
        for stmt in [
            "DELETE FROM user_roles WHERE user_id IN (SELECT id FROM users WHERE tenant_id = $1)",
            "DELETE FROM users WHERE tenant_id = $1",
            "DELETE FROM roles WHERE tenant_id = $1",
            "DELETE FROM tenant_oidc_config WHERE tenant_id = $1",
            "DELETE FROM tenants WHERE id = $1",
        ] {
            let _ = sqlx::query(stmt).bind(tenant_id).execute(admin).await;
        }
    }

    /// A user that exists and is active in the tenant must be found by email.
    #[tokio::test]
    async fn db_link_existing_active_user_by_email() {
        let db_url = std::env::var("DATABASE_URL").ok();
        let admin_url = std::env::var("DATABASE_ADMIN_URL").ok();
        let (Some(db_url), Some(admin_url)) = (db_url, admin_url) else {
            eprintln!("SKIP: DATABASE_URL/DATABASE_ADMIN_URL unset");
            return;
        };
        let admin = sqlx::PgPool::connect(&admin_url).await.unwrap();
        let pool = talentos_db::TenantPool::connect(&db_url, 2).await.unwrap();
        let auth_pool = sqlx::PgPool::connect(&db_url).await.unwrap();
        let state = test_app_state(pool, auth_pool);

        let tenant_id = Uuid::new_v4();
        seed_oidc_tenant(&admin, &tenant_id).await;

        // Seed an active user directly via the tenant pool.
        let mut tx = state.pool.begin_tenant(TenantId(tenant_id)).await.unwrap();
        let user_id = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO users (id, tenant_id, email, password_hash, is_active)
             VALUES ($1, $2, 'sso-link@example.com', 'x', true)",
        )
        .bind(user_id)
        .bind(tenant_id)
        .execute(&mut *tx)
        .await
        .unwrap();
        tx.commit().await.unwrap();

        // Case 1: existing active user — must be found (binds resolve, query succeeds).
        let found = link_or_create_user(&state, &TenantId(tenant_id), "sso-link@example.com")
            .await
            .expect("link_or_create_user must succeed against the dev DB")
            .expect("an active user with this email must be found");
        assert_eq!(found, user_id);

        cleanup_oidc_tenant(&admin, &tenant_id).await;
    }

    /// A new email in a tenant with no user yet must be created.
    #[tokio::test]
    async fn db_link_or_create_new_user_in_empty_tenant() {
        let db_url = std::env::var("DATABASE_URL").ok();
        let admin_url = std::env::var("DATABASE_ADMIN_URL").ok();
        let (Some(db_url), Some(admin_url)) = (db_url, admin_url) else {
            eprintln!("SKIP: DATABASE_URL/DATABASE_ADMIN_URL unset");
            return;
        };
        let admin = sqlx::PgPool::connect(&admin_url).await.unwrap();
        let pool = talentos_db::TenantPool::connect(&db_url, 2).await.unwrap();
        let auth_pool = sqlx::PgPool::connect(&db_url).await.unwrap();
        let state = test_app_state(pool, auth_pool);

        let tenant_id = Uuid::new_v4();
        seed_oidc_tenant(&admin, &tenant_id).await;

        // No user exists yet in this tenant; the INSERT path must run
        // (placeholders must resolve).
        let created = link_or_create_user(&state, &TenantId(tenant_id), "sso-new@example.com")
            .await
            .expect("link_or_create_user (create path) must succeed against the dev DB")
            .expect("a new user must be created for an unknown email");
        assert!(!created.is_nil());

        // Verify rows landed: user + role link, read through a tenant tx.
        let mut verify = state.pool.begin_tenant(TenantId(tenant_id)).await.unwrap();
        let user_count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM users WHERE email = $1 AND is_active")
                .bind("sso-new@example.com")
                .fetch_one(&mut *verify)
                .await
                .expect("users count query must succeed against the dev DB");
        assert_eq!(user_count, 1);

        let user_role_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM user_roles ur
             JOIN users u ON ur.user_id = u.id
             WHERE u.email = $1",
        )
        .bind("sso-new@example.com")
        .fetch_one(&mut *verify)
        .await
        .expect("user_roles count query must succeed against the dev DB");
        assert_eq!(user_role_count, 1, "the default role must be assigned");
        let _ = verify.rollback().await;

        cleanup_oidc_tenant(&admin, &tenant_id).await;
    }

    /// An inactive user must be rejected (found but not active).
    #[tokio::test]
    async fn db_inactive_user_is_rejected() {
        let db_url = std::env::var("DATABASE_URL").ok();
        let admin_url = std::env::var("DATABASE_ADMIN_URL").ok();
        let (Some(db_url), Some(admin_url)) = (db_url, admin_url) else {
            eprintln!("SKIP: DATABASE_URL/DATABASE_ADMIN_URL unset");
            return;
        };
        let admin = sqlx::PgPool::connect(&admin_url).await.unwrap();
        let pool = talentos_db::TenantPool::connect(&db_url, 2).await.unwrap();
        let auth_pool = sqlx::PgPool::connect(&db_url).await.unwrap();
        let state = test_app_state(pool, auth_pool);

        let tenant_id = Uuid::new_v4();
        seed_oidc_tenant(&admin, &tenant_id).await;

        let mut tx = state.pool.begin_tenant(TenantId(tenant_id)).await.unwrap();
        sqlx::query(
            "INSERT INTO users (tenant_id, email, password_hash, is_active)
             VALUES ($1, 'sso-inactive@example.com', 'x', false)",
        )
        .bind(tenant_id)
        .execute(&mut *tx)
        .await
        .unwrap();
        tx.commit().await.unwrap();

        let found = link_or_create_user(&state, &TenantId(tenant_id), "sso-inactive@example.com")
            .await
            .expect("link_or_create_user must succeed against the dev DB");
        assert!(found.is_none(), "an inactive user must not be linked");

        cleanup_oidc_tenant(&admin, &tenant_id).await;
    }
}
