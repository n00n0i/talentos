use thiserror::Error;

/// Crate-level error enum. Mapped to HTTP status codes only at the API gateway layer.
#[derive(Debug, Error)]
pub enum KernelError {
    #[error("tenant context missing or invalid")]
    MissingTenantContext,

    #[error("access denied: {reason}")]
    AccessDenied { reason: String },

    #[error("validation failed: {field}: {message}")]
    Validation { field: String, message: String },

    #[error("entity not found: {entity_type}/{id}")]
    NotFound { entity_type: String, id: String },

    #[error("conflict: {message}")]
    Conflict { message: String },

    #[error("internal error: {0}")]
    Internal(String),
}

impl KernelError {
    /// Used by API gateway to map to HTTP responses. Kept here so domain crates stay framework-free.
    pub fn http_status(&self) -> u16 {
        match self {
            Self::MissingTenantContext => 401,
            Self::AccessDenied { .. } => 403,
            Self::Validation { .. } => 422,
            Self::NotFound { .. } => 404,
            Self::Conflict { .. } => 409,
            Self::Internal(_) => 500,
        }
    }
}