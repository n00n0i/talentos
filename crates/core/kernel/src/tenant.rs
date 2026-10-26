use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Opaque identifiers — newtype pattern prevents mixing up IDs at compile time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TenantId(pub Uuid);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct EntityId(pub Uuid);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct UserId(pub Uuid);

impl std::fmt::Display for TenantId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::fmt::Display for EntityId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::fmt::Display for UserId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Carried through every request/workflow/activity.
/// Injected by middleware at the edge; consumed by DB layer to set RLS session variable.
#[derive(Debug, Clone)]
pub struct TenantContext {
    pub tenant_id: TenantId,
    /// None = cross-entity admin within this tenant group.
    pub entity_id: Option<EntityId>,
    pub user_id: UserId,
    pub roles: Vec<String>,
}

impl TenantContext {
    pub fn has_role(&self, role: &str) -> bool {
        self.roles.iter().any(|r| r == role)
    }
}