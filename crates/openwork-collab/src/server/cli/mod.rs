mod direct;
mod groups;
mod mailbox;
mod membership;
mod messaging;
mod reactions;
mod shared;

use std::fmt::Write as _;

use sha2::{Digest, Sha256};
use sqlx::{PgPool, Postgres, Transaction};

use crate::protocol::{CliRequest, CliResult};

use super::{auth::AgentClaims, coordination::Coordination};
use mailbox::{ack, current_inbox};
use membership::Change as MembershipChange;
use shared::{error as cli_error, valid_emoji, valid_message_body};

#[derive(Clone)]
pub struct CliDispatcher {
    pool: PgPool,
    coordination: Coordination,
}

impl CliDispatcher {
    pub fn new(pool: PgPool, coordination: Coordination) -> Self {
        Self { pool, coordination }
    }

    pub async fn execute(
        &self,
        claims: &AgentClaims,
        request: CliRequest,
    ) -> Result<CliResult, sqlx::Error> {
        if !valid_request_id(&request.request_id) {
            return Ok(cli_error(2, "INVALID_ARGUMENT: invalid request id"));
        }
        if contains_identity_flag(&request.argv) {
            return Ok(cli_error(
                2,
                "INVALID_ARGUMENT: identity flags are not accepted",
            ));
        }
        let mut transaction = self.pool.begin().await?;
        let Some(run_id) = active_run(&mut transaction, claims).await? else {
            return Ok(cli_error(3, "UNAUTHENTICATED: no active run"));
        };
        let mutates = is_mutating_command(&request.argv);
        if mutates {
            let argv_hash =
                digest(&serde_json::to_vec(&request.argv).expect("argv is serializable"));
            if let Some(result) = reserve_request(
                &mut transaction,
                &run_id,
                claims,
                &request.request_id,
                &argv_hash,
            )
            .await?
            {
                transaction.commit().await?;
                return Ok(result);
            }
        }

        let result = match request.argv.as_slice() {
            [command] if command == "inbox" => {
                current_inbox(&mut transaction, &run_id, claims).await?
            }
            [command, room_id] if command == "glance" => {
                self.glance(&mut transaction, &run_id, claims, room_id)
                    .await?
            }
            [command, room_id, separator, body]
                if command == "reply" && separator == "--" && valid_message_body(body) =>
            {
                self.reply(&mut transaction, &run_id, claims, room_id, body, None)
                    .await?
            }
            [command, room_id, held_flag, token, separator, body]
                if command == "reply"
                    && held_flag == "--held-token"
                    && separator == "--"
                    && valid_message_body(body) =>
            {
                self.reply(
                    &mut transaction,
                    &run_id,
                    claims,
                    room_id,
                    body,
                    Some(token),
                )
                .await?
            }
            [command, room_id] if command == "ack" => {
                ack(&mut transaction, &run_id, room_id).await?
            }
            [command, participant_id, separator, body]
                if command == "dm" && separator == "--" && valid_message_body(body) =>
            {
                direct::send(&mut transaction, &run_id, claims, participant_id, body).await?
            }
            [command, message_id, emoji] if command == "react" && valid_emoji(emoji) => {
                reactions::set_active(&mut transaction, &run_id, claims, message_id, emoji).await?
            }
            argv if argv.starts_with(&["group".to_string(), "create".to_string()]) => {
                match groups::parse_create(argv) {
                    Ok(command) => {
                        groups::create(&mut transaction, &run_id, claims, command).await?
                    }
                    Err(message) => cli_error(2, message),
                }
            }
            [group, action, room_id, participant_id] if group == "group" && action == "invite" => {
                membership::apply(
                    &mut transaction,
                    &run_id,
                    claims,
                    room_id,
                    participant_id,
                    MembershipChange::Invite,
                )
                .await?
            }
            [group, action, room_id] if group == "group" && action == "leave" => {
                membership::apply(
                    &mut transaction,
                    &run_id,
                    claims,
                    room_id,
                    &claims.sub,
                    MembershipChange::Leave,
                )
                .await?
            }
            [group, action, room_id, participant_id] if group == "group" && action == "kick" => {
                membership::apply(
                    &mut transaction,
                    &run_id,
                    claims,
                    room_id,
                    participant_id,
                    MembershipChange::Kick,
                )
                .await?
            }
            _ => cli_error(
                2,
                "INVALID_ARGUMENT: expected inbox, glance, reply, ack, react, dm, or group command",
            ),
        };
        if mutates {
            if result.exit_code == 0 {
                save_result(&mut transaction, &run_id, &request.request_id, &result).await?;
            } else {
                release_request(&mut transaction, &run_id, &request.request_id).await?;
            }
        }
        transaction.commit().await?;
        Ok(result)
    }
}

fn is_mutating_command(argv: &[String]) -> bool {
    match argv {
        [command] if command == "inbox" => false,
        [command, _] if command == "glance" => false,
        _ => true,
    }
}

async fn active_run(
    transaction: &mut Transaction<'_, Postgres>,
    claims: &AgentClaims,
) -> Result<Option<String>, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT r.id
         FROM collab_runs r
         JOIN collab_agents a ON a.id = r.agent_id
         JOIN collab_computers c ON c.id = a.computer_id
         WHERE r.agent_id = $1 AND r.computer_generation = $2
           AND r.status = 'running' AND a.enabled
           AND a.computer_id = 'local' AND c.status = 'online'
           AND c.daemon_generation = $2
         FOR UPDATE OF r",
    )
    .bind(&claims.sub)
    .bind(claims.generation)
    .fetch_optional(&mut **transaction)
    .await
}

async fn reserve_request(
    transaction: &mut Transaction<'_, Postgres>,
    run_id: &str,
    claims: &AgentClaims,
    request_id: &str,
    argv_hash: &str,
) -> Result<Option<CliResult>, sqlx::Error> {
    let inserted: Option<String> = sqlx::query_scalar(
        "INSERT INTO collab_cli_requests (
            run_id, request_id, agent_id, argv_hash, dedupe_key
         ) VALUES ($1, $2, $3, $4, $4)
         ON CONFLICT DO NOTHING
         RETURNING request_id",
    )
    .bind(run_id)
    .bind(request_id)
    .bind(&claims.sub)
    .bind(argv_hash)
    .fetch_optional(&mut **transaction)
    .await?;
    if inserted.is_some() {
        return Ok(None);
    }
    let existing_request: Option<(String, Option<serde_json::Value>)> = sqlx::query_as(
        "SELECT argv_hash, result
         FROM collab_cli_requests
         WHERE run_id = $1 AND request_id = $2
         FOR UPDATE",
    )
    .bind(run_id)
    .bind(request_id)
    .fetch_optional(&mut **transaction)
    .await?;
    if let Some((existing_hash, result)) = existing_request {
        if existing_hash != argv_hash {
            return Ok(Some(cli_error(
                2,
                "INVALID_ARGUMENT: request id was already used for different arguments",
            )));
        }
        return stored_result(result).map(Some);
    }
    let result: Option<serde_json::Value> = sqlx::query_scalar(
        "SELECT result FROM collab_cli_requests
         WHERE run_id = $1 AND dedupe_key = $2
         FOR UPDATE",
    )
    .bind(run_id)
    .bind(argv_hash)
    .fetch_one(&mut **transaction)
    .await?;
    stored_result(result).map(Some)
}

fn stored_result(result: Option<serde_json::Value>) -> Result<CliResult, sqlx::Error> {
    let result = result.ok_or_else(|| protocol_error("CONFLICT: CLI request is still running"))?;
    serde_json::from_value(result)
        .map_err(|error| protocol_error(&format!("PROTOCOL: invalid stored CLI result: {error}")))
}

async fn save_result(
    transaction: &mut Transaction<'_, Postgres>,
    run_id: &str,
    request_id: &str,
    result: &CliResult,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE collab_cli_requests SET result = $3
         WHERE run_id = $1 AND request_id = $2",
    )
    .bind(run_id)
    .bind(request_id)
    .bind(serde_json::to_value(result).expect("CliResult is serializable"))
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

async fn release_request(
    transaction: &mut Transaction<'_, Postgres>,
    run_id: &str,
    request_id: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM collab_cli_requests WHERE run_id = $1 AND request_id = $2")
        .bind(run_id)
        .bind(request_id)
        .execute(&mut **transaction)
        .await?;
    Ok(())
}

fn valid_request_id(request_id: &str) -> bool {
    request_id.strip_prefix("cli_").is_some_and(|suffix| {
        !suffix.is_empty()
            && suffix.len() <= 96
            && suffix
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
    })
}

fn contains_identity_flag(argv: &[String]) -> bool {
    argv.iter()
        .take_while(|argument| argument.as_str() != "--")
        .any(|argument| {
            matches!(
                argument.as_str(),
                "--as" | "--agent" | "--agent-id" | "--run-id" | "--computer-id"
            ) || [
                "--as=",
                "--agent=",
                "--agent-id=",
                "--run-id=",
                "--computer-id=",
            ]
            .iter()
            .any(|prefix| argument.starts_with(prefix))
        })
}

fn digest(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut encoded = String::with_capacity(7 + digest.len() * 2);
    encoded.push_str("sha256:");
    for byte in digest {
        let _ = write!(encoded, "{byte:02x}");
    }
    encoded
}

fn protocol_error(message: &str) -> sqlx::Error {
    sqlx::Error::Protocol(message.to_string())
}
