use std::sync::Arc;

use axum::extract::FromRef;

use talents_db::TenantPool;

#[derive(Clone)]
pub struct AppState {
    pub pool: Arc<TenantPool>,
    pub jwt_secret: Arc<[u8]>,
    pub redis: fred::clients::RedisClient,
    pub access_ttl_secs: i64,
    pub refresh_ttl_days: i64,
    pub allowed_origins: Vec<String>,
    pub started_at: std::time::Instant,
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