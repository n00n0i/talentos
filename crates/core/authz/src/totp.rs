//! Pure RFC 6238 TOTP functions (SHA-1, 6-digit, 30-second step).

use hmac::{Hmac, Mac};
use rand::RngCore;
use sha1::Sha1;

const STEP_SECONDS: u64 = 30;
const CODE_DIGITS: usize = 6;

/// Generate a new random base32-encoded TOTP secret (20 bytes, no padding).
pub fn generate_secret() -> String {
    let mut bytes = [0u8; 20];
    rand::thread_rng().fill_bytes(&mut bytes);
    base32::encode(base32::Alphabet::Rfc4648 { padding: false }, &bytes)
}

/// Build an `otpauth://` URL for the given account and secret.
pub fn otpauth_url(account: &str, secret: &str) -> String {
    format!("otpauth://totp/TalentOS:{account}?secret={secret}&issuer=TalentOS")
}

/// Verify a 6-digit TOTP code for `unix_seconds` allowing ±1 step drift.
pub fn verify(secret_b32: &str, code: &str, unix_seconds: u64) -> bool {
    if code.len() != CODE_DIGITS || !code.chars().all(|c| c.is_ascii_digit()) {
        return false;
    }
    let step = unix_seconds / STEP_SECONDS;
    for delta in [0i64, -1, 1] {
        let check_step = step.saturating_add_signed(delta);
        if let Some(expected) = code_at(secret_b32, check_step) {
            if expected.as_bytes() == code.as_bytes() {
                return true;
            }
        }
    }
    false
}

/// Compute the 6-digit code at a specific counter step.
pub fn code_at(secret_b32: &str, step: u64) -> Option<String> {
    let secret = base32::decode(base32::Alphabet::Rfc4648 { padding: false }, secret_b32)?;

    type HmacSha1 = Hmac<Sha1>;
    let mut mac = HmacSha1::new_from_slice(&secret).ok()?;
    mac.update(&step.to_be_bytes());
    let digest = mac.finalize().into_bytes();

    let offset = (digest[digest.len() - 1] & 0x0f) as usize;
    let binary = ((digest[offset] as u32 & 0x7f) << 24)
        | ((digest[offset + 1] as u32) << 16)
        | ((digest[offset + 2] as u32) << 8)
        | (digest[offset + 3] as u32);
    let code = binary % 10u32.pow(CODE_DIGITS as u32);

    Some(format!("{code:0CODE_DIGITS$}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEST_SECRET_B32: &str = "GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ";

    #[test]
    fn rfc6238_vectors() {
        let cases = [
            (59, "287082"),
            (1111111109, "081804"),
            (1111111111, "050471"),
            (1234567890, "005924"),
            (2000000000, "279037"),
        ];
        for (t, expected) in cases {
            assert_eq!(
                code_at(TEST_SECRET_B32, t / STEP_SECONDS).as_deref(),
                Some(expected),
                "wrong code for T={t}"
            );
        }
    }

    #[test]
    fn verify_accepts_current_and_adjacent_steps() {
        let step = 1_000_000u64;
        let now = step * STEP_SECONDS;
        assert!(verify(
            TEST_SECRET_B32,
            &code_at(TEST_SECRET_B32, step - 1).unwrap(),
            now
        ));
        assert!(verify(
            TEST_SECRET_B32,
            &code_at(TEST_SECRET_B32, step).unwrap(),
            now
        ));
        assert!(verify(
            TEST_SECRET_B32,
            &code_at(TEST_SECRET_B32, step + 1).unwrap(),
            now
        ));
    }

    #[test]
    fn verify_rejects_two_step_drift() {
        let step = 2_000_000u64;
        let now = step * STEP_SECONDS;
        assert!(!verify(
            TEST_SECRET_B32,
            &code_at(TEST_SECRET_B32, step - 2).unwrap(),
            now
        ));
        assert!(!verify(
            TEST_SECRET_B32,
            &code_at(TEST_SECRET_B32, step + 2).unwrap(),
            now
        ));
    }

    #[test]
    fn verify_rejects_garbage_secrets_and_codes() {
        assert!(!verify("!!!not-base32!!!", "287082", 59));
        assert!(!verify(TEST_SECRET_B32, "notdigits", 59));
        assert!(!verify(TEST_SECRET_B32, "12345", 59));
        assert!(!verify(TEST_SECRET_B32, "1234567", 59));
        assert!(!verify(TEST_SECRET_B32, "000000", 59));
    }

    #[test]
    fn code_at_round_trips_and_rejects_malformed_secret() {
        let step = 123_456u64;
        let code = code_at(TEST_SECRET_B32, step).unwrap();
        assert!(verify(TEST_SECRET_B32, &code, step * STEP_SECONDS));
        assert!(code_at("!!!", step).is_none());
    }

    #[test]
    fn generate_secret_is_valid_base32_secret() {
        let secret = generate_secret();
        assert_eq!(secret.len(), 32);
        assert!(secret
            .chars()
            .all(|c| c.is_ascii_uppercase() || ('2'..='7').contains(&c)));
        assert!(base32::decode(base32::Alphabet::Rfc4648 { padding: false }, &secret).is_some());
    }

    #[test]
    fn otpauth_url_contains_account_secret_and_issuer() {
        let url = otpauth_url("alice@example.com", TEST_SECRET_B32);
        assert!(url.starts_with("otpauth://totp/"));
        assert!(url.contains(TEST_SECRET_B32));
        assert!(url.contains("issuer=TalentOS"));
        assert!(url.contains("alice@example.com"));
    }
}
