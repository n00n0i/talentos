use thiserror::Error;

/// Errors that can occur when using the model gateway
#[derive(Error, Debug)]
pub enum ModelError {
    #[error("configuration error: {0}")]
    ConfigError(String),

    #[error("HTTP error: {0}")]
    HttpError(#[from] reqwest::Error),

    #[error("API error (status {status}): {message}")]
    ApiError { status: u16, message: String },

    #[error("rate limited: {0}")]
    RateLimited(String),

    #[error("circuit breaker open: {0}")]
    CircuitBreakerOpen(String),

    #[error("model unavailable: {0}")]
    ModelUnavailable(String),

    #[error("timeout: {0}")]
    Timeout(String),

    #[error("serialization error: {0}")]
    SerializationError(#[from] serde_json::Error),

    #[error("invalid response: {0}")]
    InvalidResponse(String),

    #[error("token counting error: {0}")]
    TokenCountError(String),
}
