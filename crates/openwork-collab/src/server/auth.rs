use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use sqlx::{Postgres, Transaction};
use uuid::Uuid;

use crate::protocol::{AgendaCandidateSet, TriggerEnvelope};

type HmacSha256 = Hmac<Sha256>;
const JWT_HEADER: &str = "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9";

#[derive(Clone)]
pub(crate) struct SigningKey(Vec<u8>);

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct AgentClaims {
    pub(crate) sub: String,
    pub(crate) runtime_session_id: String,
    pub(crate) scope: String,
    pub(crate) iat: i64,
    pub(crate) exp: i64,
    pub(crate) jti: String,
}

impl SigningKey {
    pub(crate) fn ephemeral() -> Self {
        Self(
            format!(
                "{}{}{}",
                Uuid::new_v4().simple(),
                Uuid::new_v4().simple(),
                Uuid::new_v4().simple()
            )
            .into_bytes(),
        )
    }

    pub(crate) fn mint_agent_token(
        &self,
        agent_id: &str,
        runtime_session_id: &str,
        now: i64,
    ) -> Result<(String, i64), AuthError> {
        let expires_at = now + 30 * 60;
        let claims = AgentClaims {
            sub: agent_id.to_string(),
            runtime_session_id: runtime_session_id.to_string(),
            scope: "agent".to_string(),
            iat: now,
            exp: expires_at,
            jti: Uuid::new_v4().to_string(),
        };
        let payload = URL_SAFE_NO_PAD.encode(serde_json::to_vec(&claims)?);
        let signing_input = format!("{JWT_HEADER}.{payload}");
        let signature = self.sign(signing_input.as_bytes());
        Ok((format!("{signing_input}.{signature}"), expires_at))
    }

    pub(crate) fn verify_agent_token(
        &self,
        token: &str,
        runtime_session_id: &str,
        now: i64,
    ) -> Result<AgentClaims, AuthError> {
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
            || claims.scope != "agent"
            || claims.runtime_session_id != runtime_session_id
        {
            return Err(AuthError::InvalidToken);
        }
        Ok(claims)
    }

    pub(crate) fn sign_trigger(&self, trigger: &mut TriggerEnvelope) -> Result<(), AuthError> {
        trigger.signature.clear();
        trigger.signature = self.sign(&serde_json::to_vec(trigger)?);
        Ok(())
    }

    pub(crate) fn verify_trigger(&self, trigger: &TriggerEnvelope) -> Result<(), AuthError> {
        let mut unsigned = trigger.clone();
        unsigned.signature.clear();
        self.verify(&serde_json::to_vec(&unsigned)?, &trigger.signature)
    }

    pub(crate) fn sign_agenda_candidates(
        &self,
        candidates: &mut AgendaCandidateSet,
    ) -> Result<(), AuthError> {
        candidates.signature.clear();
        candidates.signature = self.sign(&serde_json::to_vec(candidates)?);
        Ok(())
    }

    pub(crate) fn verify_agenda_candidates(
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
pub(crate) enum AuthError {
    #[error("invalid or expired credential")]
    InvalidToken,
    #[error("invalid token encoding: {0}")]
    Base64(#[from] base64::DecodeError),
    #[error("invalid token payload: {0}")]
    Json(#[from] serde_json::Error),
}

pub(crate) async fn authorize_agent_transaction(
    transaction: &mut Transaction<'_, Postgres>,
    claims: &AgentClaims,
) -> Result<(), sqlx::Error> {
    let active: bool = sqlx::query_scalar(
        "SELECT EXISTS(
            SELECT 1 FROM collab_agent_profiles
            WHERE agent_id = $1 AND archived_at IS NULL
         )",
    )
    .bind(&claims.sub)
    .fetch_one(&mut **transaction)
    .await?;
    if active {
        Ok(())
    } else {
        Err(sqlx::Error::Protocol(
            "UNAUTHENTICATED: Agent is archived or missing".to_string(),
        ))
    }
}
