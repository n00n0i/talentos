//! One-time recovery codes for TOTP 2FA.
//!
//! Codes are 8 base32 characters (A-Z, 2-7) formatted as `XXXX-XXXX`.
//! Only the SHA-256 hash of the normalized code is ever stored.

use rand::RngCore;
use sha2::{Digest, Sha256};

const CODE_LEN: usize = 8;
const GROUP_LEN: usize = 4;
const HASH_LEN: usize = 32;

/// Generate `count` fresh recovery codes.
pub fn generate_codes(count: usize) -> Vec<String> {
    (0..count).map(|_| generate_one()).collect()
}

fn generate_one() -> String {
    let mut bytes = [0u8; 5];
    rand::thread_rng().fill_bytes(&mut bytes);
    let encoded = base32::encode(base32::Alphabet::Rfc4648 { padding: false }, &bytes);
    let chars: String = encoded.chars().take(CODE_LEN).collect();
    format_code(&chars)
}

/// Normalize user input: uppercase, remove dashes and whitespace.
pub fn normalize(code: &str) -> String {
    code.to_ascii_uppercase()
        .chars()
        .filter(|c| !c.is_whitespace() && *c != '-')
        .collect()
}

/// Format an 8-character base32 string as `XXXX-XXXX`.
pub fn format_code(code: &str) -> String {
    let normalized = normalize(code);
    if normalized.len() != CODE_LEN {
        return normalized;
    }
    format!(
        "{}-{}",
        &normalized[0..GROUP_LEN],
        &normalized[GROUP_LEN..CODE_LEN]
    )
}

/// SHA-256 hash of the normalized code, suitable for database storage.
pub fn hash_code(code: &str) -> Vec<u8> {
    Sha256::digest(normalize(code).as_bytes()).to_vec()
}

/// Check whether `code` has the correct shape.
pub fn is_valid_format(code: &str) -> bool {
    let normalized = normalize(code);
    normalized.len() == CODE_LEN
        && normalized
            .chars()
            .all(|c| c.is_ascii_uppercase() || ('2'..='7').contains(&c))
}

/// Constant-time comparison of two hashes.
pub fn hashes_equal(a: &[u8], b: &[u8]) -> bool {
    if a.len() != HASH_LEN || b.len() != HASH_LEN {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b) {
        diff |= x ^ y;
    }
    diff == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_codes_are_eight_base32_chars_formatted() {
        let codes = generate_codes(10);
        assert_eq!(codes.len(), 10);
        for code in &codes {
            assert_eq!(code.len(), 9); // XXXX-XXXX
            assert!(code.contains('-'));
            assert!(is_valid_format(code));
            assert_eq!(format_code(code), *code);
        }
    }

    #[test]
    fn normalize_strips_dashes_spaces_and_lowercase() {
        assert_eq!(normalize("abcd-1234"), "ABCD1234");
        assert_eq!(normalize("  ab cd-12 34  "), "ABCD1234");
        assert_eq!(normalize("aB2C-d4E7"), "AB2CD4E7");
    }

    #[test]
    fn hash_code_is_stable_and_normalized() {
        let a = hash_code("abcd-1234");
        let b = hash_code("  ABCD1234  ");
        let c = hash_code("ABCD-1234");
        assert_eq!(a.len(), HASH_LEN);
        assert!(hashes_equal(&a, &b));
        assert!(hashes_equal(&a, &c));
    }

    #[test]
    fn invalid_formats_are_rejected() {
        assert!(!is_valid_format("123")); // too short
        assert!(!is_valid_format("ABCD0189")); // 0,1,8,9 not in base32 alphabet
        assert!(!is_valid_format("123456789")); // too long
        assert!(!is_valid_format("ABCD-123")); // wrong char (1)
    }
}
