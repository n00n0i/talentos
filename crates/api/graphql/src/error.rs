//! Standardized error format for GraphQL and MCP

use async_graphql::ErrorExtensions;
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

/// GraphQL error extension trait
pub trait GraphQLErrorExt {
    fn with_code(self, code: &str) -> async_graphql::Error;
    fn with_details(self, details: HashMap<String, serde_json::Value>) -> async_graphql::Error;
}

impl GraphQLErrorExt for async_graphql::Error {
    fn with_code(self, code: &str) -> async_graphql::Error {
        self.extend_with(|_, e| e.set("code", code))
    }

    fn with_details(self, details: HashMap<String, serde_json::Value>) -> async_graphql::Error {
        self.extend_with(move |_, e| e.set("details", details))
    }
}

/// Helper to create standardized GraphQL errors
pub fn gql_error(code: &str, message: &str) -> async_graphql::Error {
    async_graphql::Error::new(message).with_code(code)
}

pub fn gql_error_with_details(code: &str, message: &str, details: HashMap<String, serde_json::Value>) -> async_graphql::Error {
    async_graphql::Error::new(message).with_code(code).with_details(details)
}