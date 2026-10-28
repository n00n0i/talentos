use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct LoginRequest {
    pub email: String,
    pub password: String,
    pub tenant_slug: String,
    pub totp_code: Option<String>,
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
        if let Some(code) = &self.totp_code {
            let normalized = code.to_ascii_uppercase().replace(['-', ' '], "");
            let is_totp = code.len() == 6 && code.chars().all(|c| c.is_ascii_digit());
            let is_recovery = normalized.len() == 8
                && normalized
                    .chars()
                    .all(|c| c.is_ascii_uppercase() || ('2'..='7').contains(&c));
            if !is_totp && !is_recovery {
                errs.push(("totp_code", "must be 6 digits or a valid recovery code"));
            }
        }
        errs
    }
}

#[derive(Debug, Deserialize)]
pub struct RecoveryTotpBody {
    pub code: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn req(code: Option<&str>) -> LoginRequest {
        LoginRequest {
            email: "a@b.com".into(),
            password: "12345678".into(),
            tenant_slug: "acme".into(),
            totp_code: code.map(Into::into),
        }
    }

    #[test]
    fn absent_totp_code_is_valid() {
        assert!(req(None).validate().is_empty());
    }

    #[test]
    fn six_digit_totp_code_is_valid() {
        assert!(req(Some("123456")).validate().is_empty());
        assert!(req(Some("000000")).validate().is_empty());
    }

    #[test]
    fn non_six_digit_totp_code_is_invalid() {
        for code in ["12345", "1234567", "abcdef", "12345a", " 123456 "] {
            let errs = req(Some(code)).validate();
            assert!(
                errs.iter().any(|(f, _)| f == &"totp_code"),
                "code {code} should be rejected"
            );
        }
    }

    #[test]
    fn recovery_code_format_is_valid() {
        assert!(req(Some("abcd-2345")).validate().is_empty());
        assert!(req(Some("ABCD 2345")).validate().is_empty());
        assert!(req(Some("ABCD2345")).validate().is_empty());
    }
}

#[derive(Debug, Deserialize)]
pub struct RefreshRequest {
    pub refresh_token: String,
}
