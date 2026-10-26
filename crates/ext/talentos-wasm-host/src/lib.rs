//! Sandboxed tenant plugin host (Extism).
//!
//! Security model:
//! - no filesystem, no network, no env inheritance — Extism denies all of it
//!   unless a host function is registered;
//! - every plugin runs with a fuel and memory budget, so a runaway loop cannot
//!   take the process down;
//! - a plugin only receives the capabilities the tenant granted it. A host
//!   function that was not granted simply does not exist in the sandbox, so the
//!   call fails instead of silently returning empty data.

pub mod capabilities;
pub mod registry;

pub use capabilities::{Capability, CapabilitySet};
pub use registry::{InvokeOutcome, PluginError, PluginHost, PluginLimits, PluginSpec};

use talentos_kernel::TenantId;

/// Everything a plugin invocation may see about the caller.
/// Injected by the host — a plugin can never read this from environment or disk.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TenantContextPayload {
    pub tenant_id: String,
    pub entity_id: Option<String>,
    pub user_id: String,
    pub run_id: String,
}

impl TenantContextPayload {
    pub fn new(tenant_id: TenantId, run_id: uuid::Uuid) -> Self {
        Self {
            tenant_id: tenant_id.to_string(),
            entity_id: None,
            user_id: String::new(),
            run_id: run_id.to_string(),
        }
    }
}
