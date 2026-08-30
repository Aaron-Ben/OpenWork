use std::path::Path;

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use tokio::io::AsyncWriteExt;
use uuid::Uuid;

use crate::protocol::{AgendaCandidateSet, TriggerEnvelope};

type HmacSha256 = Hmac<Sha256>;
const JWT_HEADER: &str = "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9";

#[derive(Clone)]
pub struct SigningKey(Vec<u8>);

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct AgentClaims {
    pub sub: String,
    pub computer_id: String,
    pub generation: i64,
    pub scope: String,
    pub iat: i64,
    pub exp: i64,
    pub jti: String,
}

pub async fn load_or_create_signing_key(state_root: &Path) -> Result<SigningKey, std::io::Error> {
    let path = state_root.join("runtime-signing.key");
    match tokio::fs::read(&path).await {
        Ok(bytes) if bytes.len() >= 32 => return Ok(SigningKey(bytes)),
        Ok(_) => {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "runtime signing key is too short",
            ));
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    let key = format!(
        "{}{}{}",
        Uuid::new_v4().simple(),
        Uuid::new_v4().simple(),
        Uuid::new_v4().simple()
    )
    .into_bytes();
    let mut file = tokio::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .await?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        file.set_permissions(std::fs::Permissions::from_mode(0o600))
            .await?;
    }
    file.write_all(&key).await?;
    file.sync_all().await?;
    Ok(SigningKey(key))
}

impl SigningKey {
    pub fn mint_agent_token(
        &self,
        agent_id: &str,
        generation: i64,
        now: i64,
    ) -> Result<(String, i64), AuthError> {
        let expires_at = now + 30 * 60;
        let claims = AgentClaims {
            sub: agent_id.to_string(),
            computer_id: "local".to_string(),
            generation,
            scope: "runtime:agent".to_string(),
            iat: now,
            exp: expires_at,
            jti: Uuid::new_v4().to_string(),
        };
        let payload = URL_SAFE_NO_PAD.encode(serde_json::to_vec(&claims)?);
        let signing_input = format!("{JWT_HEADER}.{payload}");
        let signature = self.sign(signing_input.as_bytes());
        Ok((format!("{signing_input}.{signature}"), expires_at))
    }

    pub fn verify_agent_token(&self, token: &str, now: i64) -> Result<AgentClaims, AuthError> {
        let mut segments = token.split('.');
        let header = segments.next().ok_or(AuthError::InvalidToken)?;
        let payload = segments.next().ok_or(AuthError::InvalidToken)?;
        let signature = segments.next().ok_or(AuthError::InvalidToken)?;
        if segments.next().is_some() || header != JWT_HEADER {
            return Err(AuthError::InvalidToken);
        }
        self.verify(format!("{header}.{payload}").as_bytes(), signature)?;
        let claims: AgentClaims = serde_json::from_slice(&URL_SAFE_NO_PAD.decode(payload)?)?;
        if claims.exp <= now
            || claims.iat > now + 30
            || claims.scope != "runtime:agent"
            || claims.computer_id != "local"
        {
            return Err(AuthError::InvalidToken);
        }
        Ok(claims)
    }

    pub fn sign_trigger(&self, trigger: &mut TriggerEnvelope) -> Result<(), AuthError> {
        trigger.signature.clear();
        trigger.signature = self.sign(&serde_json::to_vec(trigger)?);
        Ok(())
    }

    pub fn verify_trigger(&self, trigger: &TriggerEnvelope) -> Result<(), AuthError> {
        let mut unsigned = trigger.clone();
        unsigned.signature.clear();
        self.verify(&serde_json::to_vec(&unsigned)?, &trigger.signature)
    }

    pub fn sign_agenda_candidates(
        &self,
        candidates: &mut AgendaCandidateSet,
    ) -> Result<(), AuthError> {
        candidates.signature.clear();
        candidates.signature = self.sign(&serde_json::to_vec(candidates)?);
        Ok(())
    }

    pub fn verify_agenda_candidates(
        &self,
        candidates: &AgendaCandidateSet,
    ) -> Result<(), AuthError> {
        let mut unsigned = candidates.clone();
        unsigned.signature.clear();
        self.verify(&serde_json::to_vec(&unsigned)?, &candidates.signature)
    }

    fn sign(&self, bytes: &[u8]) -> String {
        let mut mac = HmacSha256::new_from_slice(&self.0).expect("HMAC accepts any key length");
        mac.update(bytes);
        URL_SAFE_NO_PAD.encode(mac.finalize().into_bytes())
    }

    fn verify(&self, bytes: &[u8], signature: &str) -> Result<(), AuthError> {
        let signature = URL_SAFE_NO_PAD.decode(signature)?;
        let mut mac = HmacSha256::new_from_slice(&self.0).expect("HMAC accepts any key length");
        mac.update(bytes);
        mac.verify_slice(&signature)
            .map_err(|_| AuthError::InvalidToken)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum AuthError {
    #[error("invalid or expired token")]
    InvalidToken,
    #[error("invalid token encoding: {0}")]
    Base64(#[from] base64::DecodeError),
    #[error("invalid token payload: {0}")]
    Json(#[from] serde_json::Error),
}
