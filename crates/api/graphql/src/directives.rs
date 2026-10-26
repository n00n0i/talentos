use async_graphql::parser::types::FieldDefinition;
use async_graphql::{Name, Positioned};

/// AuthGuard directive marker — enforced at resolver level via `tenant_ctx()`.
/// SDL declaration lives in schema.rs as:
///   directive @authGuard on FIELD_DEFINITION | OBJECT
/// This module provides the Rust-side check used by resolvers.

pub fn require_role(ctx_roles: &[String], role: &str) -> Result<(), async_graphql::Error> {
    if !ctx_roles.iter().any(|r| r == role) {
        return Err(async_graphql::Error::new(format!(
            "forbidden: requires role '{}'",
            role
        )));
    }
    Ok(())
}

pub fn require_permission(
    roles: &[String],
    resource: &str,
    action: &str,
) -> Result<(), async_graphql::Error> {
    if !talentos_authz::has_permission(roles, resource, action) {
        return Err(async_graphql::Error::new(format!(
            "forbidden: {}:{} not permitted",
            resource, action
        )));
    }
    Ok(())
}

// Suppress unused warnings for positioned field def during SDL-only phase.
#[allow(dead_code)]
fn _field_marker(_: &Positioned<FieldDefinition>, _: Name) {}