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
    #[error("provider credential envelope is invalid")]
    InvalidEnvelope,
    #[error("provider credential envelope version is unsupported")]
    UnsupportedVersion,
    #[error("provider credential encryption failed")]
    EncryptionFailed,
    #[error("provider credential decryption failed")]
    DecryptionFailed,
    #[error("provider credential plaintext is invalid")]
    InvalidPlaintext,
    #[error("provider credential cipher initialization failed")]
    CipherInitializationFailed,
    #[error("{field} must not be blank")]
    BlankField { field: &'static str },
}

fn validate_non_blank(field: &'static str, value: &str) -> Result<(), ApiKeyCipherError> {
    if value.trim().is_empty() {
        Err(ApiKeyCipherError::BlankField { field })
    } else {
        Ok(())
    }
}
