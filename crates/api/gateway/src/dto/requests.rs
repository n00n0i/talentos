use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct LoginRequest {
    pub email: String,
    pub password: String,
    pub tenant_slug: String,
}

impl LoginRequest {
    /// Field-level validation per spec §4.1. Returns list of (field, message).
    pub fn validate(&self) -> Vec<(&'static str, &'static str)> {
        let mut errs = Vec::new();
        if self.email.is_empty() || self.email.len() > 255 || !self.email.contains('@') {
            errs.push(("email", "must be a valid email address"));
        }
        if self.password.len() < 8 || self.password.len() > 128 {
            errs.push(("password", "must be between 8 and 128 characters"));
        }
        if !self
            .tenant_slug
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
            || !(2..=63).contains(&self.tenant_slug.len())
        {
            errs.push(("tenant_slug", "must match [a-z0-9-]{2,63}"));
        }
        errs
    }
}

#[derive(Debug, Deserialize)]
pub struct RefreshRequest {
    pub refresh_token: String,
}
