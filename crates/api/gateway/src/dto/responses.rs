use serde::{Deserialize, Serialize};

#[derive(Serialize)]
pub struct TokenResponse {
    pub access_token: String,
    pub refresh_token: String,
    pub token_type: &'static str,
    pub expires_in: i64,
}

#[derive(Serialize)]
pub struct MeResponse {
    pub user_id: String,
    pub email: String,
    pub full_name_th: Option<String>,
    pub full_name_en: Option<String>,
    pub tenant_id: String,
    pub tenant_name: Option<String>,
    pub entity_id: Option<String>,
    pub roles: Vec<String>,
    pub permissions: Vec<String>,
    pub last_login_at: Option<String>,
}

#[derive(Serialize)]
pub struct HealthResponse {
    pub status: &'static str,
    pub version: &'static str,
    pub checks: HealthChecks,
    pub uptime_seconds: u64,
}

#[derive(Serialize)]
pub struct HealthChecks {
    pub database: &'static str,
    pub redis: &'static str,
}

/// Error envelope per spec §4.5. `details` present only on 400 validation errors.
#[derive(Serialize, Deserialize)]
pub struct ErrorResponse {
    pub error: String,
    pub message: String,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub details: Vec<FieldError>,
    pub request_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub retry_after_seconds: Option<u64>,
}

#[derive(Serialize, Deserialize)]
pub struct FieldError {
    pub field: String,
    pub message: String,
}
