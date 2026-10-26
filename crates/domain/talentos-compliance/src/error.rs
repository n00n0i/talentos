use thiserror::Error;

/// Compliance rulebook calculation and lookup errors.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum ComplianceError {
    #[error("amount cannot be negative: {field}")]
    NegativeAmount { field: String },

    #[error("invalid rate specified for {field}: {rate}")]
    InvalidRate { field: String, rate: String },

    #[error("missing configuration: {config_name}")]
    MissingConfig { config_name: String },

    #[error("period or date mismatch: {reason}")]
    PeriodMismatch { reason: String },

    #[error("statutory limit exceeded: {limit_name} limit of {max_allowed} reached, got {actual}")]
    LimitExceeded { limit_name: String, max_allowed: String, actual: String },
}
