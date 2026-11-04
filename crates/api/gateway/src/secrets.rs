//! Provider secrets in OpenBao KV v2.
//!
//! One entry per provider:
//! `secret/data/talentos/tenants/<tenant>/providers/<provider_id>`.
//! When `OPENBAO_ENABLED` is unset/false the client is absent and callers must
//! fail closed — there is no Postgres key fallback.

use reqwest::StatusCode;
use serde::Deserialize;
use uuid::Uuid;

const REQUEST_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

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
}

/// KV v2 API path for one provider's API key.
pub fn provider_secret_path(tenant: Uuid, provider: Uuid) -> String {
    format!("secret/data/talentos/tenants/{tenant}/providers/{provider}")
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
        }
    }

    /// `Ok(None)` means the provider has no stored key.
    pub async fn read_provider_key(&self, path: &str) -> Result<Option<String>, SecretError> {
        let resp = self
            .http
            .get(self.url(path))
            .header("X-Vault-Token", &self.token)
            .send()
            .await?;

        if resp.status() == StatusCode::NOT_FOUND {
            return Ok(None);
        }
        if !resp.status().is_success() {
            return Err(SecretError::UnexpectedStatus(resp.status().as_u16()));
        }

        let body: KvReadResponse = resp.json().await?;
        Ok(body
            .data
            .and_then(|d| d.data)
            .and_then(|d| d.api_key)
            .filter(|key| !key.trim().is_empty()))
    }

    pub async fn write_provider_key(
        &self,
        tenant: Uuid,
        provider: Uuid,
        api_key: &str,
    ) -> Result<(), SecretError> {
        let resp = self
            .http
            .post(self.url(&provider_secret_path(tenant, provider)))
            .header("X-Vault-Token", &self.token)
            .json(&serde_json::json!({ "data": { "api_key": api_key } }))
            .send()
            .await?;

        if !resp.status().is_success() {
            return Err(SecretError::UnexpectedStatus(resp.status().as_u16()));
        }
        Ok(())
    }

    /// Deleting a key that is already gone counts as success.
    pub async fn delete_provider_key(&self, path: &str) -> Result<(), SecretError> {
        let resp = self
            .http
            .delete(self.url(path))
            .header("X-Vault-Token", &self.token)
            .send()
            .await?;

        if resp.status().is_success() || resp.status() == StatusCode::NOT_FOUND {
            return Ok(());
        }
        Err(SecretError::UnexpectedStatus(resp.status().as_u16()))
    }

    fn url(&self, path: &str) -> String {
        format!("{}/v1/{}", self.addr, path)
    }
}

#[derive(Deserialize)]
struct KvReadResponse {
    data: Option<KvReadData>,
}

#[derive(Deserialize)]
struct KvReadData {
    data: Option<ProviderSecret>,
}

#[derive(Deserialize)]
struct ProviderSecret {
    api_key: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    const TENANT: &str = "0f8fad5b-d9cb-469f-a165-70867728950e";
    const PROVIDER: &str = "9b2c4a1e-7d3f-4a8b-9c1d-2e3f4a5b6c7d";

    #[test]
    fn path_is_kv_v2_layout() {
        let tenant = Uuid::parse_str(TENANT).unwrap();
        let provider = Uuid::parse_str(PROVIDER).unwrap();
        assert_eq!(
            provider_secret_path(tenant, provider),
            format!("secret/data/talentos/tenants/{TENANT}/providers/{PROVIDER}")
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
