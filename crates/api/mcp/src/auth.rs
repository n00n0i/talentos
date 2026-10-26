//! TenantContext extraction & validation for MCP tool calls.
//!
//! Every tool handler MUST call [`extract_tenant_context`] before touching any
//! data. The returned `TenantContext` is then passed to
//! [`TenantPool::with_tenant_tx`] which binds `app.current_tenant_id` so RLS
//! enforces tenant isolation at the database level.
//!
//! The context is carried through MCP request `extensions` (HTTP headers equivalent).
//! It is injected by the gateway/edge layer when the MCP session is established.
//!
//! ## Security
//! - Missing context → `ErrorData::invalid_params` (never silently proceed).
//! - Malformed UUIDs → `ErrorData::invalid_params`.
//! - The function is the ONLY entry point; tools must not construct contexts themselves.

use rmcp::model::Extensions;
use talentos_kernel::TenantContext;

/// Errors returned when the MCP request metadata cannot produce a valid TenantContext.
#[derive(Debug, thiserror::Error)]
pub enum TenantContextError {
    #[error("missing tenant_id in request metadata")]
    MissingTenantId,

    #[error("invalid tenant_id: {0} (expected UUID v7)")]
    InvalidTenantId(String),

    #[error("missing user_id in request metadata")]
    MissingUserId,

    #[error("invalid user_id: {0} (expected UUID v7)")]
    InvalidUserId(String),

    #[error("missing entity_id in request metadata")]
    MissingEntityId,

    #[error("invalid entity_id: {0} (expected UUID v7)")]
    InvalidEntityId(String),
}

/// Extract a [`TenantContext`] from MCP request extensions.
///
/// The gateway stores the context as a serialized JSON blob under the
/// `talents.tenant_context` extension key. This function deserializes it
/// and validates every UUID field.
///
/// Returns `Err` if any required field is missing or malformed — the tool
/// handler should propagate this as a structured JSON error response.
pub fn extract_tenant_context(extensions: &Extensions) -> Result<TenantContext, TenantContextError> {
    let blob = extensions
        .get::<TenantContextBlob>()
        .ok_or(TenantContextError::MissingTenantId)?;

    let tenant_id = parse_uuid(&blob.tenant_id, "tenant_id")
        .ok_or_else(|| TenantContextError::InvalidTenantId(blob.tenant_id.clone()))?;
    let user_id = parse_uuid(&blob.user_id, "user_id")
        .ok_or_else(|| TenantContextError::InvalidUserId(blob.user_id.clone()))?;
    let entity_id = parse_uuid(&blob.entity_id, "entity_id")
        .ok_or_else(|| TenantContextError::InvalidEntityId(blob.entity_id.clone()))?;

    Ok(TenantContext {
        tenant_id,
        entity_id: Some(entity_id),
        user_id,
        roles: blob.roles,
    })
}

/// Convenience: extract from a `&rmcp::service::RequestContext`.
pub fn extract_from_request_context(
    ctx: &rmcp::service::RequestContext<rmcp::RoleServer>,
) -> Result<TenantContext, TenantContextError> {
    extract_tenant_context(&ctx.extensions)
}

/// Convert a [`TenantContextError`] into a structured RMCP [`ErrorData`].
///
/// The error is returned as a JSON object in the `data` field so clients
/// receive a structured response rather than a bare message string.
pub fn to_error_data(err: TenantContextError) -> rmcp::ErrorData {
    let code = match &err {
        TenantContextError::MissingTenantId
        | TenantContextError::MissingUserId
        | TenantContextError::MissingEntityId => rmcp::model::ErrorCode::INVALID_PARAMS,
        TenantContextError::InvalidTenantId(_)
        | TenantContextError::InvalidUserId(_)
        | TenantContextError::InvalidEntityId(_) => rmcp::model::ErrorCode::INVALID_PARAMS,
    };

    let data = serde_json::json!({
        "error": err.to_string(),
        "code": "INVALID_TENANT_CONTEXT",
    });

    rmcp::ErrorData::new(code, err.to_string(), Some(data))
}

/// Wire format carried in MCP request extensions.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TenantContextBlob {
    pub tenant_id: String,
    pub user_id: String,
    pub entity_id: String,
    pub roles: Vec<String>,
}

impl TenantContextBlob {
    /// Build a blob from a validated context (used by the gateway when
    /// stamping the MCP session extensions).
    pub fn from_context(ctx: &TenantContext) -> Self {
        Self {
            tenant_id: ctx.tenant_id.to_string(),
            user_id: ctx.user_id.to_string(),
            entity_id: ctx
                .entity_id
                .map(|e| e.to_string())
                .unwrap_or_default(),
            roles: ctx.roles.clone(),
        }
    }

    /// Insert this blob into the given extensions map so tool handlers can
    /// extract it later.
    pub fn insert_into(&self, extensions: &mut rmcp::model::Extensions) {
        extensions.insert(self.clone());
    }
}

fn parse_uuid(s: &str, _field: &str) -> Option<uuid::Uuid> {
    uuid::Uuid::parse_str(s).ok()
}