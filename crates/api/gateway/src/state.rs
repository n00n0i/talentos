use std::sync::Arc;

use axum::extract::FromRef;

use talentos_db::TenantPool;

use crate::handlers::oidc;

#[derive(Clone)]
pub struct AppState {
    pub pool: Arc<TenantPool>,
    /// BYPASSRLS pool: login/refresh bootstrap only (tenant unknown there).
    pub auth_pool: sqlx::PgPool,
    pub jwt_secret: Arc<[u8]>,
    pub redis: fred::clients::Client,
    pub access_ttl_secs: i64,
    pub refresh_ttl_days: i64,
    pub allowed_origins: Vec<String>,
    pub graphql_schema: std::sync::Arc<talentos_graphql::TalentSchema>,
    pub memory: std::sync::Arc<talentos_memory::MemoryStore>,
    /// `None` only when PII_ENCRYPTION_KEY is unset (dev); production must set it.
    pub pii: Option<std::sync::Arc<talentos_db::pii::PiiCipher>>,
    /// Base URL of the app (e.g. `http://localhost:3000`), used to build
    /// absolute `redirect_uri` values for the OIDC callback. `None` in tests.
    pub base_url: Option<String>,
    /// Shared HTTP client for outbound OIDC discovery / token exchange.
    pub http: reqwest::Client,
    /// Shared OIDC discovery cache, keyed by issuer.
    pub oidc_cache: oidc::DiscoveryCache,
    /// `None` when OpenBao is disabled; there is no Postgres key fallback.
    pub secrets: Option<Arc<crate::secrets::SecretsClient>>,
    pub started_at: std::time::Instant,
}

impl AppState {
    /// E-signature provider name from `E_SIGNATURE_PROVIDER`; `None` means
    /// the internal hash-based acknowledgement is used.
    pub fn esign_provider(&self) -> Option<String> {
        std::env::var("E_SIGNATURE_PROVIDER")
            .ok()
            .filter(|v| !v.trim().is_empty())
            .map(|v| v.trim().to_string())
    }
}

impl FromRef<AppState> for Arc<TenantPool> {
    fn from_ref(state: &AppState) -> Self {
        state.pool.clone()
    }
}

impl FromRef<AppState> for Arc<[u8]> {
    fn from_ref(state: &AppState) -> Self {
        state.jwt_secret.clone()
    }
}
