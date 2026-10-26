//! Application-level encryption for employee PII (national id, SSO number,
//! bank account). Ciphertext is stored as `v1:<hex(nonce || ciphertext)>`;
//! values without the prefix are legacy plaintext and are read as-is so a
//! rollout does not break existing rows.

use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Nonce};
use rand::RngCore;

const PREFIX: &str = "v1:";
const NONCE_LEN: usize = 12;

#[derive(Debug, thiserror::Error)]
pub enum PiiError {
    #[error("PII_ENCRYPTION_KEY must be 64 hex characters (32 bytes)")]
    BadKey,
    #[error("PII value is malformed")]
    Malformed,
    #[error("PII value could not be decrypted")]
    Decrypt,
    #[error("PII value could not be encrypted")]
    Encrypt,
}

pub struct PiiCipher {
    cipher: Aes256Gcm,
}

impl PiiCipher {
    /// `None` when the key is unset (dev); `Err` when it is set but invalid.
    pub fn from_env() -> Result<Option<Self>, PiiError> {
        match std::env::var("PII_ENCRYPTION_KEY") {
            Ok(key) if !key.trim().is_empty() => Self::from_hex(key.trim()).map(Some),
            _ => Ok(None),
        }
    }

    pub fn from_hex(key: &str) -> Result<Self, PiiError> {
        let bytes = hex::decode(key).map_err(|_| PiiError::BadKey)?;
        if bytes.len() != 32 {
            return Err(PiiError::BadKey);
        }
        let cipher = Aes256Gcm::new_from_slice(&bytes).map_err(|_| PiiError::BadKey)?;
        Ok(Self { cipher })
    }

    pub fn encrypt(&self, plaintext: &str) -> Result<String, PiiError> {
        let mut nonce_bytes = [0u8; NONCE_LEN];
        rand::thread_rng().fill_bytes(&mut nonce_bytes);
        let nonce = Nonce::from_slice(&nonce_bytes);
        let ciphertext = self
            .cipher
            .encrypt(nonce, plaintext.as_bytes())
            .map_err(|_| PiiError::Encrypt)?;
        let mut blob = nonce_bytes.to_vec();
        blob.extend_from_slice(&ciphertext);
        Ok(format!("{PREFIX}{}", hex::encode(blob)))
    }

    /// Decrypt a stored value. Legacy plaintext (no prefix) is returned intact.
    pub fn decrypt(&self, stored: &str) -> Result<String, PiiError> {
        let Some(body) = stored.strip_prefix(PREFIX) else {
            return Ok(stored.to_string());
        };
        let blob = hex::decode(body).map_err(|_| PiiError::Malformed)?;
        if blob.len() <= NONCE_LEN {
            return Err(PiiError::Malformed);
        }
        let (nonce_bytes, ciphertext) = blob.split_at(NONCE_LEN);
        let plaintext = self
            .cipher
            .decrypt(Nonce::from_slice(nonce_bytes), ciphertext)
            .map_err(|_| PiiError::Decrypt)?;
        String::from_utf8(plaintext).map_err(|_| PiiError::Malformed)
    }

    pub fn encrypt_opt(&self, value: Option<&str>) -> Result<Option<String>, PiiError> {
        value
            .map(|v| {
                if v.trim().is_empty() {
                    Ok(None)
                } else {
                    self.encrypt(v).map(Some)
                }
            })
            .transpose()
            .map(Option::flatten)
    }

    pub fn decrypt_opt(&self, value: Option<&str>) -> Result<Option<String>, PiiError> {
        value.map(|v| self.decrypt(v)).transpose()
    }
}

pub fn is_encrypted(value: &str) -> bool {
    value.starts_with(PREFIX)
}

/// Last four digits for display. Never returns the full value.
pub fn mask_secret(value: &str) -> String {
    let digits: String = value.chars().filter(|c| c.is_ascii_digit()).collect();
    if digits.len() <= 4 {
        return "****".to_string();
    }
    format!("****{}", &digits[digits.len() - 4..])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cipher() -> PiiCipher {
        PiiCipher::from_hex(&"11".repeat(32)).expect("key")
    }

    #[test]
    fn round_trips_and_never_stores_plaintext() {
        let c = cipher();
        let stored = c.encrypt("1234567890123").unwrap();
        assert!(stored.starts_with("v1:"));
        assert!(!stored.contains("1234567890123"));
        assert_eq!(c.decrypt(&stored).unwrap(), "1234567890123");
    }

    #[test]
    fn wrong_key_and_tampering_fail() {
        let stored = cipher().encrypt("secret").unwrap();
        let other = PiiCipher::from_hex(&"22".repeat(32)).unwrap();
        assert!(other.decrypt(&stored).is_err());

        let mut tampered = stored.clone();
        tampered.pop();
        tampered.push(if stored.ends_with('0') { '1' } else { '0' });
        assert!(cipher().decrypt(&tampered).is_err());
    }

    #[test]
    fn legacy_plaintext_still_reads() {
        assert_eq!(cipher().decrypt("0123456789").unwrap(), "0123456789");
        assert!(!is_encrypted("0123456789"));
    }

    #[test]
    fn mask_keeps_only_last_four_digits() {
        assert_eq!(mask_secret("123-4-56789-0"), "****7890");
        assert_eq!(mask_secret("123"), "****");
    }

    #[test]
    fn rejects_short_keys() {
        assert!(PiiCipher::from_hex("abcd").is_err());
    }
}
