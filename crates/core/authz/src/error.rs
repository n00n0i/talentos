use thiserror::Error;

#[derive(Debug, Error)]
pub enum AuthError {
    #[error("invalid credentials")]
    InvalidCredentials,

    #[error("token expired")]
    TokenExpired,

    #[error("token malformed")]
    TokenMalformed,

    #[error("token signature invalid")]
    TokenSignatureInvalid,

    #[error("insufficient permission: {reason}")]
    InsufficientPermission { reason: String },

    #[error("account disabled")]
    AccountDisabled,

    #[error("rate limited")]
    RateLimited,

    #[error("internal error: {0}")]
    Internal(String),
}

impl AuthError {
    pub fn http_status(&self) -> u16 {
        match self {
            Self::InvalidCredentials | Self::AccountDisabled => 401,
            Self::TokenExpired | Self::TokenMalformed | Self::TokenSignatureInvalid => 401,
            Self::InsufficientPermission { .. } => 403,
            Self::RateLimited => 429,
            Self::Internal(_) => 500,
        }
    }

    pub fn error_code(&self) -> &str {
        match self {
            Self::InvalidCredentials => "unauthorized",
            Self::TokenExpired => "token_expired",
            Self::TokenMalformed => "token_malformed",
            Self::TokenSignatureInvalid => "token_signature_invalid",
            Self::InsufficientPermission { .. } => "insufficient_permission",
            Self::AccountDisabled => "unauthorized",
            Self::RateLimited => "rate_limited",
            Self::Internal(_) => "internal_error",
        }
    }
}