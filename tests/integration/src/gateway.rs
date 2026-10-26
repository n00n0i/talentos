//! Gateway process management for integration tests.

use std::process::{Command, Stdio};
use std::time::Duration;
use tokio::time::sleep;
use reqwest::Client;
use anyhow::Result;
use sqlx::PgPool;

/// A handle to a running gateway process.
pub struct GatewayHandle {
    pub base_url: String,
    pub client: reqwest::Client,
    pub pool: PgPool,
    child: std::process::Child,
}

impl GatewayHandle {
    /// Spawn a new gateway process on a random available port.
    pub async fn spawn() -> Result<Self> {
        // Find a free port
        let port = portpicker::pick_unused_port().expect("no free port");
        let base_url = format!("http://127.0.0.1:{}", port);

        let database_url = std::env::var("DATABASE_URL")
            .unwrap_or_else(|_| "postgres://talentos:talentos_dev_secret@localhost:5432/talentos".into());
        let redis_url = std::env::var("REDIS_URL")
            .unwrap_or_else(|_| "redis://:redis_dev_secret@localhost:6379".into());
        let jwt_secret = std::env::var("AUTH_JWT_SECRET")
            .unwrap_or_else(|_| "dev-secret-change-me-in-prod-minimum-32-bytes-okay!!".into());

        // Create a pool for test fixtures
        let pool = PgPool::connect(&database_url).await?;

        let mut child = Command::new("cargo")
            .args(["run", "-p", "talentos-gateway"])
            .env("DATABASE_URL", &database_url)
            .env("REDIS_URL", &redis_url)
            .env("AUTH_JWT_SECRET", &jwt_secret)
            .env("GATEWAY_PORT", port.to_string())
            .env("AUTH_ACCESS_TTL_SECS", "900")
            .env("AUTH_REFRESH_TTL_DAYS", "7")
            .env("AUTH_ALLOWED_ORIGINS", "http://localhost:3000")
            .env("RUST_LOG", "info")
            .current_dir(std::env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| "."))
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;

        let client = Client::builder()
            .timeout(Duration::from_secs(10))
            .cookie_store(true)
            .build()?;

        let handle = Self { base_url, client, pool, child };

        // Wait for gateway to be healthy
        handle.wait_healthy().await?;
        Ok(handle)
    }

    async fn wait_healthy(&self) -> Result<()> {
        let health_url = format!("{}/health", self.base_url);
        for _ in 0..30 {
            if let Ok(resp) = self.client.get(&health_url).send().await {
                if resp.status().is_success() {
                    return Ok(());
                }
            }
            sleep(Duration::from_millis(500)).await;
        }
        anyhow::bail!("Gateway did not become healthy in time");
    }

    pub fn url(&self, path: &str) -> String {
        format!("{}{}", self.base_url, path)
    }

    pub fn client(&self) -> &reqwest::Client {
        &self.client
    }

    pub fn pool(&self) -> &PgPool {
        &self.pool
    }
}

impl Drop for GatewayHandle {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Convenience: spawn gateway once for all tests in a module.
#[macro_export]
macro_rules! with_gateway {
    ($test_fn:expr) => {{
        let handle = $crate::gateway::GatewayHandle::spawn().await?;
        $test_fn(&handle).await?;
        Ok(())
    }};
}