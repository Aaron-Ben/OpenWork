use std::fmt::Write as _;

use sqlx::{Postgres, Transaction};
use uuid::Uuid;

use crate::protocol::{CliResult, CliSideEffect};

use super::{
    CliDispatcher, cli_error,
    shared::{insert_message, mark_delivery_action},
};
use crate::server::{auth::AgentClaims, coordination::HeldBinding};

impl CliDispatcher {
    pub(super) async fn reply(
        &self,
        transaction: &mut Transaction<'_, Postgres>,
        run_id: &str,
        claims: &AgentClaims,
        room_id: &str,
        body: &str,
        held_token: Option<&str>,
    ) -> Result<CliResult, sqlx::Error> {
        let row: Option<(i64, String)> = sqlx::query_as(
            "SELECT d.up_to_seq, r.kind
             FROM collab_run_deliveries d
             JOIN collab_rooms r ON r.id = d.room_id
             JOIN collab_room_members own
               ON own.room_id = d.room_id AND own.participant_id = $3
             WHERE d.run_id = $1 AND d.room_id = $2
             FOR UPDATE OF r",
        )
        .bind(run_id)
        .bind(room_id)
        .bind(&claims.sub)
        .fetch_optional(&mut **transaction)
        .await?;
        let Some((compose_anchor, room_kind)) = row else {
            return Ok(cli_error(4, "NOT_FOUND: room is not in the active run"));
        };
        let member_count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM collab_room_members WHERE room_id = $1")
                .bind(room_id)
                .fetch_one(&mut **transaction)
                .await?;
        if room_kind == "direct" && held_token.is_some() {
            return Ok(cli_error(
                2,
                "INVALID_ARGUMENT: HELD token is not valid for a direct room",
            ));
        }
        if room_kind == "group" && member_count > 2 {
            let peer_max = peer_max(transaction, room_id, &claims.sub, compose_anchor).await?;
            match held_token {
                None if peer_max.is_some() => {
                    return self
                        .hold_reply(
                            transaction,
                            run_id,
                            claims,
                            room_id,
                            compose_anchor,
                            peer_max.expect("checked above"),
                        )
                        .await;
                }
                Some(token) => {
                    let binding = match self
                        .coordination
                        .consume_held(&claims.sub, room_id, token)
                        .await
                    {
                        Ok(Some(binding)) => binding,
                        Ok(None) => {
                            return Ok(cli_error(
                                10,
                                "HELD: retry token is invalid, expired, or already consumed",
                            ));
                        }
                        Err(error) => {
                            tracing::warn!(%error, %room_id, "HELD token consumption failed closed");
                            return Ok(cli_error(
                                11,
                                "RATE_LIMITED: coordination is temporarily unavailable",
                            ));
                        }
                    };
                    if binding.agent_id != claims.sub
                        || binding.run_id != run_id
                        || binding.room_id != room_id
                        || binding.computer_generation != claims.generation
                        || binding.compose_anchor != compose_anchor
                    {
                        return Ok(cli_error(10, "HELD: retry token does not match this run"));
                    }
                    if peer_max.is_some_and(|sequence| sequence > binding.shown_peer_max) {
                        return self
                            .hold_reply(
                                transaction,
                                run_id,
                                claims,
                                room_id,
                                compose_anchor,
                                peer_max.expect("checked above"),
                            )
                            .await;
                    }
                }
                None => {}
            }
        }
        let message_id = format!("msg_{}", Uuid::new_v4().simple());
        let sequence = insert_message(
            transaction,
            room_id,
            &claims.sub,
            "normal",
            body,
            Some(&message_id),
        )
        .await?;
        mark_delivery_action(transaction, run_id, room_id).await?;
        Ok(CliResult {
            text: format!("Published message {message_id}"),
            exit_code: 0,
            side_effects: vec![CliSideEffect::MessagePublished {
                room_id: room_id.to_string(),
                message_id,
                sequence,
            }],
        })
    }

    async fn hold_reply(
        &self,
        transaction: &mut Transaction<'_, Postgres>,
        run_id: &str,
        claims: &AgentClaims,
        room_id: &str,
        compose_anchor: i64,
        peer_max: i64,
    ) -> Result<CliResult, sqlx::Error> {
        let messages: Vec<(i64, String, String)> = sqlx::query_as(
            "SELECT sequence, author_id, body
             FROM (
                 SELECT sequence, author_id, body
                 FROM collab_messages
                 WHERE room_id = $1 AND sequence > $2 AND sequence <= $3
                   AND author_id <> $4
                 ORDER BY sequence DESC LIMIT 50
             ) recent
             ORDER BY sequence",
        )
        .bind(room_id)
        .bind(compose_anchor)
        .bind(peer_max)
        .bind(&claims.sub)
        .fetch_all(&mut **transaction)
        .await?;
        let binding = HeldBinding {
            agent_id: claims.sub.clone(),
            run_id: run_id.to_string(),
            room_id: room_id.to_string(),
            computer_generation: claims.generation,
            compose_anchor,
            shown_peer_max: peer_max,
        };
        let token = match self.coordination.issue_held(&binding).await {
            Ok(token) => token,
            Err(error) => {
                tracing::warn!(%error, %room_id, "HELD token issuance failed closed");
                return Ok(cli_error(
                    11,
                    "RATE_LIMITED: coordination is temporarily unavailable",
                ));
            }
        };
        if let Err(error) = self
            .coordination
            .record_seen(&claims.sub, room_id, peer_max)
            .await
        {
            tracing::warn!(%error, %room_id, "seen update failed open");
        }
        let mut text = String::from("HELD: room changed after this run began.\nNew messages:\n");
        for (sequence, author_id, body) in messages {
            let _ = writeln!(text, "[{sequence}] {author_id}: {body}");
        }
        text.push_str("Re-read and reconsider, then retry with:\n--held-token ");
        text.push_str(&token);
        Ok(cli_error(10, &text))
    }
}

async fn peer_max(
    transaction: &mut Transaction<'_, Postgres>,
    room_id: &str,
    agent_id: &str,
    compose_anchor: i64,
) -> Result<Option<i64>, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT MAX(sequence) FROM collab_messages
         WHERE room_id = $1 AND sequence > $2 AND author_id <> $3",
    )
    .bind(room_id)
    .bind(compose_anchor)
    .bind(agent_id)
    .fetch_one(&mut **transaction)
    .await
}
