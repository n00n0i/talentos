//! TalentOS Integration Test Harness
//!
//! Provides fixtures, helpers, and a running gateway for integration tests.
//! Run with: `cargo nextest run --test integration --features integration`

pub mod fixtures;
pub mod gateway;
pub mod auth;
pub mod payroll;
pub mod rls;
pub mod health;

use std::sync::Once;
use tracing_subscriber::{EnvFilter, fmt};

static INIT: Once = Once::new();

/// Initialize tracing for tests (only once per process).
pub fn init_tracing() {
    INIT.call_once(|| {
        let _ = fmt()
            .with_env_filter(EnvFilter::from_default_env().add_directive("talentos=debug".parse().unwrap()))
            .with_test_writer()
            .try_init();
    });
}

/// Generate a unique test email to avoid collisions across parallel runs.
pub fn unique_email(prefix: &str) -> String {
    use rand::Rng;
    let suffix: u64 = rand::rng().random();
    format!("{}_test_{}@example.com", prefix, suffix)
}

/// Generate a unique tenant slug.
pub fn unique_tenant_slug(prefix: &str) -> String {
    use rand::Rng;
    let suffix: u64 = rand::rng().random();
    format!("{}-test-{}", prefix, suffix)
}