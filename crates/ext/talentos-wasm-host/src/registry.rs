//! Plugin loading and invocation.

use std::collections::HashMap;
use std::time::Duration;

use extism::{Manifest, Plugin, Wasm};
use talentos_kernel::TenantId;

use crate::capabilities::{Capability, CapabilitySet};
use crate::TenantContextPayload;

#[derive(Debug, thiserror::Error)]
pub enum PluginError {
    #[error("plugin '{0}' is not registered for this tenant")]
    NotFound(String),
    #[error("plugin '{plugin}' requires capability '{capability}' which was not granted")]
    MissingCapability { plugin: String, capability: String },
    #[error("plugin '{plugin}' failed: {message}")]
    Invoke { plugin: String, message: String },
    #[error("plugin '{plugin}' returned invalid JSON: {message}")]
    BadOutput { plugin: String, message: String },
    #[error("invalid plugin: {0}")]
    Invalid(String),
}

/// Resource budget applied to every plugin instance.
#[derive(Debug, Clone)]
pub struct PluginLimits {
    /// WebAssembly pages (64 KiB each). 256 pages ≈ 16 MiB.
    pub memory_pages: u32,
    pub timeout: Duration,
}

impl Default for PluginLimits {
    fn default() -> Self {
        Self {
            memory_pages: 256,
            timeout: Duration::from_secs(2),
        }
    }
}

/// A plugin package as delivered by ops / a tenant admin.
#[derive(Debug, Clone)]
pub struct PluginSpec {
    pub name: String,
    pub wasm: Vec<u8>,
    /// Capabilities the package declares it needs. The host refuses to load it
    /// unless the tenant granted all of them.
    pub requires: CapabilitySet,
}

#[derive(Debug)]
pub struct InvokeOutcome {
    pub output: serde_json::Value,
    pub plugin: String,
}

/// Registry keyed by (tenant, plugin name).
pub struct PluginHost {
    limits: PluginLimits,
    plugins: HashMap<(TenantId, String), (PluginSpec, CapabilitySet)>,
}

impl PluginHost {
    pub fn new(limits: PluginLimits) -> Self {
        Self {
            limits,
            plugins: HashMap::new(),
        }
    }

    /// Register a plugin for a tenant, enforcing the capability check up front.
    pub fn register(
        &mut self,
        tenant_id: TenantId,
        spec: PluginSpec,
        granted: CapabilitySet,
    ) -> Result<(), PluginError> {
        if spec.wasm.is_empty() || !spec.wasm.starts_with(b"\0asm") {
            return Err(PluginError::Invalid(format!(
                "{}: not a WebAssembly module",
                spec.name
            )));
        }
        for cap in spec.requires.iter() {
            if !granted.allows(*cap) {
                return Err(PluginError::MissingCapability {
                    plugin: spec.name.clone(),
                    capability: cap.as_str().to_string(),
                });
            }
        }
        self.plugins
            .insert((tenant_id, spec.name.clone()), (spec, granted));
        Ok(())
    }

    pub fn unregister(&mut self, tenant_id: TenantId, name: &str) {
        self.plugins.remove(&(tenant_id, name.to_string()));
    }

    pub fn list(&self, tenant_id: TenantId) -> Vec<String> {
        let mut names: Vec<String> = self
            .plugins
            .keys()
            .filter(|(t, _)| *t == tenant_id)
            .map(|(_, n)| n.clone())
            .collect();
        names.sort();
        names
    }

    /// Invoke `function` on the tenant's plugin with `input`, injecting the
    /// caller's context when (and only when) the tenant granted that capability.
    pub fn invoke(
        &mut self,
        tenant_id: TenantId,
        name: &str,
        function: &str,
        input: serde_json::Value,
    ) -> Result<InvokeOutcome, PluginError> {
        let (spec, granted) = self
            .plugins
            .get(&(tenant_id, name.to_string()))
            .ok_or_else(|| PluginError::NotFound(name.to_string()))?;

        let mut payload = input;
        if granted.allows(Capability::TenantContext) {
            let ctx = TenantContextPayload::new(tenant_id, uuid::Uuid::new_v4());
            if let Some(obj) = payload.as_object_mut() {
                obj.insert(
                    "tenant".to_string(),
                    serde_json::to_value(ctx).unwrap_or_default(),
                );
            }
        }

        let manifest = Manifest::new([Wasm::data(spec.wasm.clone())])
            .with_memory_max(self.limits.memory_pages)
            .with_timeout(self.limits.timeout);

        // `with_wasi = true`: wasm32-wasip1 guests import a few WASI calls.
        // Extism's shim provides them without filesystem or network access.
        let mut plugin = Plugin::new(manifest, [], true).map_err(|e| PluginError::Invoke {
            plugin: name.to_string(),
            message: e.to_string(),
        })?;

        let output: String = plugin
            .call(
                function,
                serde_json::to_string(&payload).unwrap_or_default(),
            )
            .map_err(|e| PluginError::Invoke {
                plugin: name.to_string(),
                message: e.to_string(),
            })?;

        let value: serde_json::Value =
            serde_json::from_str(&output).map_err(|e| PluginError::BadOutput {
                plugin: name.to_string(),
                message: e.to_string(),
            })?;

        Ok(InvokeOutcome {
            output: value,
            plugin: name.to_string(),
        })
    }
}

/// Locate a compiled example plugin. Returns `None` when the wasm target has not
/// been built, so tests skip instead of failing on a machine without it.
pub fn example_plugin_bytes() -> Option<Vec<u8>> {
    if let Ok(path) = std::env::var("TALENTOS_TEST_PLUGIN") {
        return std::fs::read(path).ok();
    }
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").ok()?;
    let root = std::path::Path::new(&manifest_dir).ancestors().nth(3)?;
    let path = root.join(
        "plugins/bonus-calculator-thai/target/wasm32-wasip1/release/bonus_calculator_thai.wasm",
    );
    std::fs::read(path).ok()
}
