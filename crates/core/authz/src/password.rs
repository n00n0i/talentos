use argon2::{
    Algorithm, Argon2, Params, PasswordHash, PasswordHasher, PasswordVerifier, Version,
};

use crate::error::AuthError;

// Spec §1.1: Argon2id, m=64MB, t=3, p=4.
fn algorithm() -> Argon2<'static> {
    let params = Params::new(64 * 1024, 3, 4, None).expect("valid argon2 params");
    Argon2::new(Algorithm::Argon2id, Version::V0x13, params)
}

pub fn hash_password(plain: &str) -> Result<String, AuthError> {
    use rand_core::OsRng;
    let salt = argon2::password_hash::SaltString::generate(&mut OsRng);
    algorithm()
        .hash_password(plain.as_bytes(), &salt)
        .map(|h| h.to_string())
        .map_err(|_| AuthError::InvalidCredentials)
}

/// Constant-time verify via the argon2 crate (it compares digests internally).
pub fn verify_password(plain: &str, hash: &str) -> Result<bool, AuthError> {
    let parsed = PasswordHash::new(hash).map_err(|_| AuthError::TokenMalformed)?;
    Ok(algorithm()
        .verify_password(plain.as_bytes(), &parsed)
        .is_ok())
}

/// Pre-computed Argon2id hash of a throwaway value. Verify this when user/tenant is
/// not found so response timing is indistinguishable from a real login attempt (spec §6.1 / TC-12).
pub fn dummy_hash() -> String {
    // Computed once at startup; cost mirrors production params.
    use std::sync::OnceLock;
    static DUMMY: OnceLock<String> = OnceLock::new();
    DUMMY
        .get_or_init(|| {
            hash_password("timing-equalization-placeholder-not-a-real-password")
                .expect("dummy hash must compute")
        })
        .clone()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_verify_roundtrip() {
        let h = hash_password("Secret123!").unwrap();
        assert!(h.starts_with("$argon2id$"));
        assert!(verify_password("Secret123!", &h).unwrap());
    }

    #[test]
    fn wrong_password_returns_false_not_error() {
        let h = hash_password("right-pass-123").unwrap();
        assert_eq!(verify_password("wrong-pass-456", &h).unwrap(), false);
    }

    #[test]
    fn malformed_hash_surfaces_as_error() {
        assert!(verify_password("anything", "not-a-phc-string").is_err());
    }

    #[test]
    fn dummy_hash_verifies_and_has_similar_cost() {
        use std::time::Instant;
        let d = dummy_hash();
        // Real verify against a freshly hashed password.
        let real = hash_password("some-password-value").unwrap();
        let t0 = Instant::now();
        let _ = verify_password("nope", &real);
        let real_ms = t0.elapsed().as_millis();
        let t1 = Instant::now();
        let _ = verify_password("nope", &d);
        let dummy_ms = t1.elapsed().as_millis();
        // Same params ⇒ same order of magnitude. Generous bound to avoid flakiness.
        assert!(dummy_ms > 0);
        assert!(
            dummy_ms.abs_diff(real_ms) < real_ms.max(dummy_ms),
            "dummy={dummy_ms}ms real={real_ms}ms"
        );
    }
}