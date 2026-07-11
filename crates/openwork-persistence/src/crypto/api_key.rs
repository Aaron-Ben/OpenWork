use std::{fmt, sync::Arc};

use aes_gcm::{
    Aes256Gcm, Nonce,
    aead::{Aead, Generate, KeyInit, Payload},
};
use base64::prelude::{BASE64_STANDARD, BASE64_URL_SAFE_NO_PAD, Engine as _};
use thiserror::Error;
use zeroize::{Zeroize, Zeroizing};

pub const API_KEY_ENCRYPTION_KEY_ENV: &str = "OPENWORK_API_KEY_ENCRYPTION_KEY";
const ENVELOPE_VERSION: &str = "v1";
const NONCE_LENGTH: usize = 12;
const TAG_LENGTH: usize = 16;

#[derive(Clone)]
pub struct ApiKeyCipher {
    key: Arc<Zeroizing<[u8; 32]>>,
}

impl ApiKeyCipher {
    pub fn from_env() -> Result<Self, ApiKeyCipherError> {
        let encoded = std::env::var(API_KEY_ENCRYPTION_KEY_ENV).map_err(|_| {
            ApiKeyCipherError::MissingKey {
                variable: API_KEY_ENCRYPTION_KEY_ENV,
            }
        })?;
        Self::from_base64(encoded.trim())
    }

    pub fn from_base64(encoded: &str) -> Result<Self, ApiKeyCipherError> {
        let mut decoded = Zeroizing::new(
            BASE64_STANDARD
                .decode(encoded)
                .map_err(|_| ApiKeyCipherError::InvalidKeyEncoding)?,
        );
        if decoded.len() != 32 {
            return Err(ApiKeyCipherError::InvalidKeyLength {
                actual: decoded.len(),
            });
        }
        let mut key = [0_u8; 32];
        key.copy_from_slice(&decoded);
        decoded.zeroize();
        Ok(Self::from_key(key))
    }

    pub fn from_key(key: [u8; 32]) -> Self {
        Self {
            key: Arc::new(Zeroizing::new(key)),
        }
    }

    pub fn encrypt(&self, provider_id: &str, api_key: &str) -> Result<String, ApiKeyCipherError> {
        validate_non_blank("provider_id", provider_id)?;
        validate_non_blank("api_key", api_key)?;
        let cipher = self.cipher()?;
        let nonce = Nonce::generate();
        let ciphertext = cipher
            .encrypt(
                &nonce,
                Payload {
                    msg: api_key.as_bytes(),
                    aad: provider_id.as_bytes(),
                },
            )
            .map_err(|_| ApiKeyCipherError::EncryptionFailed)?;
        let mut envelope = Vec::with_capacity(NONCE_LENGTH + ciphertext.len());
        envelope.extend_from_slice(&nonce);
        envelope.extend_from_slice(&ciphertext);
        Ok(format!(
            "{ENVELOPE_VERSION}:{}",
            BASE64_URL_SAFE_NO_PAD.encode(envelope)
        ))
    }

    pub fn decrypt(&self, provider_id: &str, encrypted: &str) -> Result<String, ApiKeyCipherError> {
        validate_non_blank("provider_id", provider_id)?;
        let (version, payload) = encrypted
            .split_once(':')
            .ok_or(ApiKeyCipherError::InvalidEnvelope)?;
        if version != ENVELOPE_VERSION {
            return Err(ApiKeyCipherError::UnsupportedVersion);
        }
        let mut decoded = Zeroizing::new(
            BASE64_URL_SAFE_NO_PAD
                .decode(payload)
                .map_err(|_| ApiKeyCipherError::InvalidEnvelope)?,
        );
        if decoded.len() < NONCE_LENGTH + TAG_LENGTH {
            return Err(ApiKeyCipherError::InvalidEnvelope);
        }
        let (nonce_bytes, ciphertext) = decoded.split_at(NONCE_LENGTH);
        let nonce = Nonce::try_from(nonce_bytes).map_err(|_| ApiKeyCipherError::InvalidEnvelope)?;
        let plaintext = self
            .cipher()?
            .decrypt(
                &nonce,
                Payload {
                    msg: ciphertext,
                    aad: provider_id.as_bytes(),
                },
            )
            .map_err(|_| ApiKeyCipherError::DecryptionFailed)?;
        decoded.zeroize();
        String::from_utf8(plaintext).map_err(|error| {
            let mut bytes = error.into_bytes();
            bytes.zeroize();
            ApiKeyCipherError::InvalidPlaintext
        })
    }

    fn cipher(&self) -> Result<Aes256Gcm, ApiKeyCipherError> {
        Aes256Gcm::new_from_slice(self.key.as_ref().as_ref())
            .map_err(|_| ApiKeyCipherError::CipherInitializationFailed)
    }
}

impl fmt::Debug for ApiKeyCipher {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("ApiKeyCipher([REDACTED])")
    }
}

#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum ApiKeyCipherError {
    #[error("missing API key encryption key: {variable}")]
    MissingKey { variable: &'static str },
    #[error("API key encryption key must be standard base64")]
    InvalidKeyEncoding,
    #[error("API key encryption key must decode to 32 bytes, got {actual}")]
    InvalidKeyLength { actual: usize },
    #[error("API key encryption input is invalid: {field}")]
    InvalidInput { field: &'static str },
    #[error("API key cipher initialization failed")]
    CipherInitializationFailed,
    #[error("API key encryption envelope is invalid")]
    InvalidEnvelope,
    #[error("API key encryption envelope version is unsupported")]
    UnsupportedVersion,
    #[error("API key encryption failed")]
    EncryptionFailed,
    #[error("API key decryption failed")]
    DecryptionFailed,
    #[error("decrypted API key is not valid UTF-8")]
    InvalidPlaintext,
}

fn validate_non_blank(field: &'static str, value: &str) -> Result<(), ApiKeyCipherError> {
    if value.trim().is_empty() {
        return Err(ApiKeyCipherError::InvalidInput { field });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cipher(byte: u8) -> ApiKeyCipher {
        ApiKeyCipher::from_key([byte; 32])
    }

    #[test]
    fn encrypts_with_unique_nonces_and_round_trips() {
        let cipher = cipher(7);
        let first = cipher.encrypt("prov-a", "sk-secret").unwrap();
        let second = cipher.encrypt("prov-a", "sk-secret").unwrap();

        assert_ne!(first, second);
        assert!(!first.contains("sk-secret"));
        assert_eq!(cipher.decrypt("prov-a", &first).unwrap(), "sk-secret");
        assert_eq!(cipher.decrypt("prov-a", &second).unwrap(), "sk-secret");
    }

    #[test]
    fn ciphertext_is_bound_to_provider_id_and_master_key() {
        let encrypted = cipher(7).encrypt("prov-a", "sk-secret").unwrap();

        assert_eq!(
            cipher(7).decrypt("prov-b", &encrypted),
            Err(ApiKeyCipherError::DecryptionFailed)
        );
        assert_eq!(
            cipher(8).decrypt("prov-a", &encrypted),
            Err(ApiKeyCipherError::DecryptionFailed)
        );
    }

    #[test]
    fn rejects_tampered_or_malformed_envelopes() {
        let cipher = cipher(7);
        let mut encrypted = cipher.encrypt("prov-a", "sk-secret").unwrap();
        encrypted.push('A');

        assert!(cipher.decrypt("prov-a", &encrypted).is_err());
        assert_eq!(
            cipher.decrypt("prov-a", "v2:abcd"),
            Err(ApiKeyCipherError::UnsupportedVersion)
        );
        assert_eq!(
            cipher.decrypt("prov-a", "not-an-envelope"),
            Err(ApiKeyCipherError::InvalidEnvelope)
        );
    }

    #[test]
    fn validates_base64_master_key_length_without_exposing_key() {
        let encoded = BASE64_STANDARD.encode([9_u8; 32]);
        let cipher = ApiKeyCipher::from_base64(&encoded).unwrap();
        assert_eq!(format!("{cipher:?}"), "ApiKeyCipher([REDACTED])");
        assert!(matches!(
            ApiKeyCipher::from_base64("not-base64"),
            Err(ApiKeyCipherError::InvalidKeyEncoding)
        ));
        assert!(matches!(
            ApiKeyCipher::from_base64(&BASE64_STANDARD.encode([1_u8; 16])),
            Err(ApiKeyCipherError::InvalidKeyLength { actual: 16 })
        ));
    }
}
