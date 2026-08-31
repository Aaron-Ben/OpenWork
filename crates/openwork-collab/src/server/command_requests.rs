use std::fmt::Write as _;

use sha2::{Digest, Sha256};
use sqlx::{Postgres, Transaction};

use crate::protocol::{AgentCommandResponse, entity_id};

use super::auth::AgentClaims;

pub(crate) struct CommandRequests;

impl CommandRequests {
    pub(crate) fn valid_id(request_id: &str) -> bool {
        request_id.len() == 36
            && request_id.starts_with("req-")
            && request_id[4..]
                .bytes()
                .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
    }

    pub(crate) fn semantic_hash(bytes: &[u8]) -> String {
        let digest = Sha256::digest(bytes);
        let mut encoded = String::with_capacity(7 + digest.len() * 2);
        encoded.push_str("sha256:");
        for byte in digest {
            let _ = write!(encoded, "{byte:02x}");
        }
        encoded
    }

    pub(crate) async fn reserve_in(
        transaction: &mut Transaction<'_, Postgres>,
        run_id: &str,
        claims: &AgentClaims,
        request_id: &str,
        semantic_hash: &str,
    ) -> Result<Option<AgentCommandResponse>, sqlx::Error> {
        let inserted: Option<String> = sqlx::query_scalar(
            "INSERT INTO collab_command_requests (
                id, runtime_session_id, run_id, request_id, actor_id, semantic_hash
             ) VALUES ($1, $2, $3, $4, $5, $6)
             ON CONFLICT DO NOTHING RETURNING id",
        )
        .bind(entity_id("cmd"))
        .bind(&claims.runtime_session_id)
        .bind(run_id)
        .bind(request_id)
        .bind(&claims.sub)
        .bind(semantic_hash)
        .fetch_optional(&mut **transaction)
        .await?;
        if inserted.is_some() {
            return Ok(None);
        }
        let existing: Option<(String, Option<serde_json::Value>)> = sqlx::query_as(
            "SELECT semantic_hash, result FROM collab_command_requests
             WHERE run_id = $1 AND request_id = $2 FOR UPDATE",
        )
        .bind(run_id)
        .bind(request_id)
        .fetch_optional(&mut **transaction)
        .await?;
        let Some((existing_hash, result)) = existing else {
            return Err(sqlx::Error::Protocol(
                "CONFLICT: command request collision".to_string(),
            ));
        };
        if existing_hash != semantic_hash {
            return Ok(Some(error(
                "CONFLICT",
                "request id was already used for a different command",
            )));
        }
        let Some(result) = result else {
            return Ok(Some(error("CONFLICT", "command request is still running")));
        };
        serde_json::from_value(result).map(Some).map_err(|error| {
            sqlx::Error::Protocol(format!("invalid stored command result: {error}"))
        })
    }

    pub(crate) async fn save_in(
        transaction: &mut Transaction<'_, Postgres>,
        run_id: &str,
        request_id: &str,
        result: &AgentCommandResponse,
    ) -> Result<(), sqlx::Error> {
        sqlx::query(
            "UPDATE collab_command_requests SET result = $3
             WHERE run_id = $1 AND request_id = $2",
        )
        .bind(run_id)
        .bind(request_id)
        .bind(serde_json::to_value(result).expect("AgentCommandResponse is serializable"))
        .execute(&mut **transaction)
        .await?;
        Ok(())
    }
}

fn error(code: &str, message: &str) -> AgentCommandResponse {
    AgentCommandResponse {
        result: crate::protocol::AgentCommandResult::Error {
            code: code.to_string(),
            message: message.to_string(),
        },
        effects: Vec::new(),
    }
}
