use thiserror::Error;

/// Errors that can occur when using the model gateway
#[derive(Error, Debug)]
pub enum ModelError {
    #[error("Configuration error: {0}")]
    ConfigError(String),

    #[error("HTTP error: {0}")]
    HttpError(#[from] reqwest::Error),

    #[error("API error (status {status}): {message}")]
    ApiError { status: u16, message: String },

    #[error("Rate limited: {0}")]
    RateLimited(String),

    #[error("Circuit breaker open: {0}")]
    CircuitBreakerOpen(String),

    #[error("Model unavailable: {0}")]
    ModelUnavailable(String),

    #[error("Timeout: {0}")]
    Timeout(String),

    #[error("Serialization error: {0}")]
    SerializationError(#[from] serde_json::Error),

    #[error("Redis error: {0}")]
    RedisError(#[from] fred::error::Error),

    #[error("Invalid response: {0}")]
    InvalidResponse(String),

    #[error("Token counting error: {0}")]
    TokenCountError(String),
}

impl ModelError {
    /// Check if error is retryable
    pub fn is_retryable(&self) -> bool {
        matches!(
            self,
            ModelError::ApiError { status: 500..=599, .. }
                | ModelError::Timeout(_)
                | ModelError::RateLimited(_)
                | ModelError::HttpError(_)
        )
    }

    /// Check if error indicates circuit breaker should trip
    pub fn is_server_error(&self) -> bool {
        matches!(self, ModelError::ApiError { status: 500..=599, .. })
    }
}