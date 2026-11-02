//! Provider secrets in OpenBao KV v2.
//!
//! One entry per tenant: `secret/data/talentos/tenants/<tenant>/ai`.
//! When `OPENBAO_ENABLED` is unset/false the client is absent and callers must
//! report `secrets_backend: none` — there is no Postgres fallback.

// Consumed by the AI settings/chat handlers in Tasks 3–5.
#![allow(dead_code)]

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

const CACHE_TTL: Duration = Duration::from_secs(60);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AiSecret {
    pub provider: String,
    pub base_url: Option<String>,
    pub model: String,
    pub api_key: String,
}

// Never let a stray `{:?}` write the key into a log line.
impl std::fmt::Debug for AiSecret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AiSecret")
            .field("provider", &self.provider)
            .field("base_url", &self.base_url)
            .field("model", &self.model)
            .field("api_key", &"<redacted>")
            .finish()
    }
}

#[derive(Debug, thiserror::Error)]
pub enum SecretError {
    #[error("OpenBao is not configured: {0} is missing")]
    NotConfigured(&'static str),
    #[error("OpenBao request failed: {0}")]
    Http(#[from] reqwest::Error),
    #[error("OpenBao returned HTTP {0}")]
    UnexpectedStatus(u16),
    #[error("OpenBao returned an unexpected response")]
    Malformed,
}

#[derive(Clone)]
pub struct SecretsClient {
    addr: String,
    token: String,
    http: reqwest::Client,
    cache: Arc<Mutex<HashMap<Uuid, CacheEntry>>>,
}

#[derive(Clone)]
struct CacheEntry {
    fetched_at: Instant,
    value: Option<AiSecret>,
}

/// KV v2 API path for a tenant's AI provider secret.
pub fn ai_secret_path(tenant: Uuid) -> String {
    format!("secret/data/talentos/tenants/{tenant}/ai")
}

impl SecretsClient {
    /// `None` means the secret backend is disabled (tests, dev without OpenBao).
    pub fn from_env() -> Result<Option<Self>, SecretError> {
        let enabled = std::env::var("OPENBAO_ENABLED")
            .map(|v| matches!(v.trim().to_ascii_lowercase().as_str(), "true" | "1"))
            .unwrap_or(false);
        if !enabled {
            return Ok(None);
        }
        let addr = std::env::var("OPENBAO_ADDR")
            .ok()
            .filter(|v| !v.trim().is_empty())
            .ok_or(SecretError::NotConfigured("OPENBAO_ADDR"))?;
        let token = std::env::var("OPENBAO_TOKEN")
            .ok()
            .filter(|v| !v.trim().is_empty())
            .ok_or(SecretError::NotConfigured("OPENBAO_TOKEN"))?;
        Ok(Some(Self::new(addr, token)))
    }

    pub fn new(addr: impl Into<String>, token: impl Into<String>) -> Self {
        let http = reqwest::Client::builder()
            .timeout(REQUEST_TIMEOUT)
            .build()
            .unwrap_or_default();
        Self {
            addr: addr.into().trim_end_matches('/').to_string(),
            token: token.into(),
            http,
            cache: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub async fn read_ai_secret(&self, tenant: Uuid) -> Result<Option<AiSecret>, SecretError> {
        if let Some(hit) = self.cache_get(tenant) {
            return Ok(hit);
        }

        let resp = self
            .http
            .get(self.url(tenant))
            .header("X-Vault-Token", &self.token)
            .send()
            .await?;

        if resp.status() == reqwest::StatusCode::NOT_FOUND {
            self.cache_put(tenant, None);
            return Ok(None);
        }
        if !resp.status().is_success() {
            return Err(SecretError::UnexpectedStatus(resp.status().as_u16()));
        }

        let body: KvReadResponse = resp.json().await?;
        let secret = body
            .data
            .and_then(|d| d.data)
            .ok_or(SecretError::Malformed)?;
        self.cache_put(tenant, Some(secret.clone()));
        Ok(Some(secret))
    }

    pub async fn write_ai_secret(
        &self,
        tenant: Uuid,
        secret: &AiSecret,
    ) -> Result<(), SecretError> {
        let resp = self
            .http
            .post(self.url(tenant))
            .header("X-Vault-Token", &self.token)
            .json(&serde_json::json!({ "data": secret }))
            .send()
            .await?;

        if !resp.status().is_success() {
            return Err(SecretError::UnexpectedStatus(resp.status().as_u16()));
        }
        self.cache_put(tenant, Some(secret.clone()));
        Ok(())
    }

    fn url(&self, tenant: Uuid) -> String {
        format!("{}/v1/{}", self.addr, ai_secret_path(tenant))
    }

    fn cache_get(&self, tenant: Uuid) -> Option<Option<AiSecret>> {
        let cache = self.cache.lock().unwrap_or_else(|e| e.into_inner());
        cache
            .get(&tenant)
            .and_then(|entry| (entry.fetched_at.elapsed() < CACHE_TTL).then(|| entry.value.clone()))
    }

    fn cache_put(&self, tenant: Uuid, value: Option<AiSecret>) {
        let mut cache = self.cache.lock().unwrap_or_else(|e| e.into_inner());
        cache.insert(
            tenant,
            CacheEntry {
                fetched_at: Instant::now(),
                value,
            },
        );
    }
}

#[derive(Deserialize)]
struct KvReadResponse {
    data: Option<KvReadData>,
}

#[derive(Deserialize)]
struct KvReadData {
    data: Option<AiSecret>,
}

#[cfg(test)]
mod tests {
    use super::*;

    const TENANT: &str = "0f8fad5b-d9cb-469f-a165-70867728950e";

    #[test]
    fn path_is_kv_v2_layout() {
        let tenant = Uuid::parse_str(TENANT).unwrap();
        assert_eq!(
            ai_secret_path(tenant),
            format!("secret/data/talentos/tenants/{TENANT}/ai")
        );
    }

    #[test]
    fn disabled_when_flag_unset_or_false() {
        let previous = std::env::var_os("OPENBAO_ENABLED");

        std::env::remove_var("OPENBAO_ENABLED");
        assert!(SecretsClient::from_env().unwrap().is_none());

        std::env::set_var("OPENBAO_ENABLED", "false");
        assert!(SecretsClient::from_env().unwrap().is_none());

        match previous {
            Some(value) => std::env::set_var("OPENBAO_ENABLED", value),
            None => std::env::remove_var("OPENBAO_ENABLED"),
        }
    }
}
