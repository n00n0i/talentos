use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Anomaly {
    pub code: String,
    pub severity: String, // "warning" or "error"
    pub message: String,
}

impl Anomaly {
    pub fn new(code: &str, severity: &str, message: &str) -> Self {
        Self {
            code: code.to_string(),
            severity: severity.to_string(),
            message: message.to_string(),
        }
    }
}