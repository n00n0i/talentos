//! Plugin capability grants.

use std::collections::BTreeSet;

use talentos_kernel::TenantId;

/// What a plugin is allowed to do. Everything is denied by default.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum Capability {
    /// Host passes the caller's tenant/entity/user context into the sandbox.
    TenantContext,
    /// Plugin may call `audit_log` to append an audit entry.
    AuditLog,
    /// Plugin may read the tenant's public payslip template config.
    PayslipTemplate,
}

impl Capability {
    pub fn as_str(&self) -> &'static str {
        match self {
            Capability::TenantContext => "tenant_context",
            Capability::AuditLog => "audit_log",
            Capability::PayslipTemplate => "payslip_template",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "tenant_context" => Capability::TenantContext,
            "audit_log" => Capability::AuditLog,
            "payslip_template" => Capability::PayslipTemplate,
            _ => return None,
        })
    }
}

/// Grants for a single (tenant, plugin) pair.
#[derive(Debug, Clone, Default)]
pub struct CapabilitySet {
    grants: BTreeSet<Capability>,
}

impl CapabilitySet {
    pub fn empty() -> Self {
        Self::default()
    }

    pub fn of(caps: impl IntoIterator<Item = Capability>) -> Self {
        Self {
            grants: caps.into_iter().collect(),
        }
    }

    pub fn allows(&self, cap: Capability) -> bool {
        self.grants.contains(&cap)
    }

    pub fn iter(&self) -> impl Iterator<Item = &Capability> {
        self.grants.iter()
    }
}

/// Request to load a plugin for one tenant.
#[derive(Debug, Clone)]
pub struct PluginGrant {
    pub tenant_id: TenantId,
    pub name: String,
    pub capabilities: CapabilitySet,
}
