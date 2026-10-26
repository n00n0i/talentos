use std::sync::Arc;

use async_graphql::Context;
use axum::http::HeaderMap;
use talents_db::TenantPool;
use talents_kernel::{TenantContext, TenantId, UserId, EntityId};

/// Shared state injected into every GraphQL request.
pub struct GraphQLContext {
    pub pool: Arc<TenantPool>,
}

/// Tenant context extracted from request extensions (set by gateway middleware)
pub struct RequestTenantContext {
    pub tenant_id: TenantId,
    pub entity_id: Option<EntityId>,
    pub user_id: UserId,
    pub roles: Vec<String>,
}

impl async_graphql::FromRequest for GraphQLContext {
    type Err = anyhow::Error;
    type Future = futures_util::future::Ready<Result<Self, Self::Err>>;

    fn from_request(req: &async_graphql::http::GraphiQLSource) -> Self::Future {
        // For SDL introspection, return minimal context
        // Real tenant context is injected via middleware in gateway
        futures_util::future::ready(Ok(Self {
            pool: Arc::new(TenantPool::new_uninitialized()),
        }))
    }
}

/// Helper: extract TenantContext from the async-graphql request data.
pub fn tenant_ctx<'a>(ctx: &'a Context<'a>) -> Result<&'a TenantContext, async_graphql::Error> {
    ctx.data::<TenantContext>()
        .map_err(|_| async_graphql::Error::new("unauthorized: missing tenant context"))
}

/// Helper: extract shared pool.
pub fn pool(ctx: &Context<'_>) -> Result<&Arc<TenantPool>, async_graphql::Error> {
    ctx.data::<Arc<TenantPool>>()
        .map_err(|_| async_graphql::Error::new("internal: missing db pool"))
}

/// Extract TenantContext from Axum request headers (used by gateway middleware)
pub fn extract_tenant_context_from_headers(headers: &HeaderMap) -> Result<TenantContext, anyhow::Error> {
    let tenant_id = headers
        .get("x-tenant-id")
        .and_then(|h| h.to_str().ok())
        .and_then(|s| uuid::Uuid::parse_str(s).ok())
        .map(TenantId)
        .ok_or_else(|| anyhow::anyhow!("missing x-tenant-id header"))?;

    let user_id = headers
        .get("x-user-id")
        .and_then(|h| h.to_str().ok())
        .and_then(|s| uuid::Uuid::parse_str(s).ok())
        .map(UserId)
        .ok_or_else(|| anyhow::anyhow!("missing x-user-id header"))?;

    let entity_id = headers
        .get("x-entity-id")
        .and_then(|h| h.to_str().ok())
        .and_then(|s| uuid::Uuid::parse_str(s).ok())
        .map(EntityId);

    let roles = headers
        .get("x-roles")
        .and_then(|h| h.to_str().ok())
        .map(|s| s.split(',').map(|s| s.trim().to_string()).collect())
        .unwrap_or_default();

    Ok(TenantContext {
        tenant_id,
        entity_id,
        user_id,
        roles,
    })
}