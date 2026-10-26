//! Standardized error format for MCP

use rmcp::ErrorData;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Standard error response format
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorResponse {
    pub code: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<HashMap<String, serde_json::Value>>,
}

impl ErrorResponse {
    pub fn new(code: &str, message: &str) -> Self {
        Self {
            code: code.to_string(),
            message: message.to_string(),
            details: None,
        }
    }

    pub fn with_details(mut self, details: HashMap<String, serde_json::Value>) -> Self {
        self.details = Some(details);
        self
    }

    /// Convert to RMCP ErrorData
    pub fn to_error_data(&self) -> ErrorData {
        let code = match self.code.as_str() {
            "INVALID_TENANT_CONTEXT" => rmcp::model::ErrorCode::INVALID_PARAMS,
            "UNAUTHORIZED" => rmcp::model::ErrorCode::INVALID_REQUEST,
            "FORBIDDEN" => rmcp::model::ErrorCode::INTERNAL_ERROR,
            "NOT_FOUND" => rmcp::model::ErrorCode::INVALID_PARAMS,
            "VALIDATION_ERROR" => rmcp::model::ErrorCode::INVALID_PARAMS,
            "INTERNAL_ERROR" => rmcp::model::ErrorCode::INTERNAL_ERROR,
            "RATE_LIMITED" => rmcp::model::ErrorCode::INTERNAL_ERROR,
            "MISSING_CONFIRMATION" => rmcp::model::ErrorCode::INVALID_PARAMS,
            "INVALID_STATE" => rmcp::model::ErrorCode::INVALID_PARAMS,
            "INVALID_DATE_RANGE" => rmcp::model::ErrorCode::INVALID_PARAMS,
            "MISSING_ENTITY_ID" => rmcp::model::ErrorCode::INVALID_PARAMS,
            _ => rmcp::model::ErrorCode::INTERNAL_ERROR,
        };

        let data = serde_json::json!({
            "error": self.message,
            "code": self.code,
            "details": self.details,
        });

        ErrorData::new(code, self.message.clone(), Some(data))
    }
}

/// Error codes used across GraphQL and MCP
pub mod codes {
    pub const INVALID_TENANT_CONTEXT: &str = "INVALID_TENANT_CONTEXT";
    pub const UNAUTHORIZED: &str = "UNAUTHORIZED";
    pub const FORBIDDEN: &str = "FORBIDDEN";
    pub const NOT_FOUND: &str = "NOT_FOUND";
    pub const VALIDATION_ERROR: &str = "VALIDATION_ERROR";
    pub const INTERNAL_ERROR: &str = "INTERNAL_ERROR";
    pub const RATE_LIMITED: &str = "RATE_LIMITED";
    pub const MISSING_CONFIRMATION: &str = "MISSING_CONFIRMATION";
    pub const INVALID_STATE: &str = "INVALID_STATE";
    pub const INVALID_DATE_RANGE: &str = "INVALID_DATE_RANGE";
    pub const MISSING_ENTITY_ID: &str = "MISSING_ENTITY_ID";
    pub const MODEL_UNAVAILABLE: &str = "MODEL_UNAVAILABLE";
    pub const CIRCUIT_BREAKER_OPEN: &str = "CIRCUIT_BREAKER_OPEN";
}

/// Helper to create standardized MCP errors
pub fn mcp_error(code: &str, message: &str) -> ErrorData {
    ErrorResponse::new(code, message).to_error_data()
}

pub fn mcp_error_with_details(code: &str, message: &str, details: HashMap<String, serde_json::Value>) -> ErrorData {
    ErrorResponse::new(code, message).with_details(details).to_error_data()
}