use std::fmt::Write as _;

use sqlx::{Postgres, Transaction};

use crate::protocol::{CliResult, CliSideEffect};

use super::{CliDispatcher, cli_error};
use crate::server::auth::AgentClaims;

impl CliDispatcher {
    pub(super) async fn glance(
        &self,
        transaction: &mut Transaction<'_, Postgres>,
        run_id: &str,
        claims: &AgentClaims,
        room_id: &str,
    ) -> Result<CliResult, sqlx::Error> {
        let compose_anchor: Option<i64> = sqlx::query_scalar(
            "SELECT d.up_to_seq
             FROM collab_run_deliveries d
             JOIN collab_room_members rm
               ON rm.room_id = d.room_id AND rm.participant_id = $3
             WHERE d.run_id = $1 AND d.room_id = $2",
        )
        .bind(run_id)
        .bind(room_id)
        .bind(&claims.sub)
        .fetch_optional(&mut **transaction)
        .await?;
        let Some(compose_anchor) = compose_anchor else {
            return Ok(cli_error(4, "NOT_FOUND: room is not in the active run"));
        };
        let messages: Vec<(i64, String, String)> = sqlx::query_as(
            "SELECT sequence, author_id, body
             FROM (
                 SELECT sequence, author_id, body
                 FROM collab_messages
                 WHERE room_id = $1 AND sequence > $2 AND author_id <> $3
                 ORDER BY sequence DESC LIMIT 50
             ) recent
             ORDER BY sequence",
        )
        .bind(room_id)
        .bind(compose_anchor)
        .bind(&claims.sub)
        .fetch_all(&mut **transaction)
        .await?;
        let roster: Vec<(String, String, String, bool)> = sqlx::query_as(
            "SELECT p.id, p.display_name, p.kind, COALESCE(a.enabled, FALSE)
             FROM collab_room_members rm
             JOIN collab_participants p ON p.id = rm.participant_id
             LEFT JOIN collab_agents a ON a.id = p.id
             WHERE rm.room_id = $1
             ORDER BY p.kind, p.display_name, p.id",
        )
        .bind(room_id)
        .fetch_all(&mut **transaction)
        .await?;
        if let Some(peer_max) = messages.last().map(|message| message.0)
            && let Err(error) = self
                .coordination
                .record_seen(&claims.sub, room_id, peer_max)
                .await
        {
            tracing::warn!(%error, %room_id, "glance seen update failed open");
        }
        let mut text = format!("Room {room_id} after compose anchor {compose_anchor}:\nRoster:\n");
        for (participant_id, display_name, kind, enabled) in roster {
            let state = if kind == "agent" {
                if enabled { ", enabled" } else { ", disabled" }
            } else {
                ""
            };
            let _ = writeln!(text, "- {participant_id} ({kind}{state}): {display_name}");
        }
        text.push_str("New peer messages:\n");
        if messages.is_empty() {
            text.push_str("No new peer messages.");
        } else {
            for (sequence, author_id, body) in messages {
                let _ = writeln!(text, "[{sequence}] {author_id}: {body}");
            }
        }
        Ok(CliResult {
            text,
            exit_code: 0,
            side_effects: Vec::new(),
        })
    }
}

pub(super) async fn current_inbox(
    transaction: &mut Transaction<'_, Postgres>,
    run_id: &str,
    claims: &AgentClaims,
) -> Result<CliResult, sqlx::Error> {
    let carried_over: bool = sqlx::query_scalar(
        "SELECT inbox_carried_over FROM collab_runs
         WHERE id = $1 AND agent_id = $2 AND computer_generation = $3
           AND status = 'running'",
    )
    .bind(run_id)
    .bind(&claims.sub)
    .bind(claims.generation)
    .fetch_one(&mut **transaction)
    .await?;
    let rows: Vec<(String, i64, i64, i64, String, String)> = sqlx::query_as(
        "SELECT d.room_id, d.from_seq, d.up_to_seq,
                m.sequence, m.author_id, m.body
         FROM collab_run_deliveries d
         JOIN collab_runs r ON r.id = d.run_id
         JOIN collab_messages m ON m.room_id = d.room_id
           AND m.sequence BETWEEN d.from_seq AND d.up_to_seq
         WHERE d.run_id = $1 AND r.agent_id = $2
           AND r.computer_generation = $3 AND r.status = 'running'
           AND m.author_id <> $2
         ORDER BY d.room_id, m.sequence",
    )
    .bind(run_id)
    .bind(&claims.sub)
    .bind(claims.generation)
    .fetch_all(&mut **transaction)
    .await?;
    let mut text = format!("Current run inbox:\ncarried_over: {carried_over}\n");
    for (room_id, from_seq, up_to_seq, sequence, author_id, body) in rows {
        let _ = writeln!(
            text,
            "room {room_id} delivery {from_seq}..{up_to_seq}\n[{sequence}] {author_id}: {body}"
        );
    }
    Ok(CliResult {
        text,
        exit_code: 0,
        side_effects: Vec::new(),
    })
}

pub(super) async fn ack(
    transaction: &mut Transaction<'_, Postgres>,
    run_id: &str,
    room_id: &str,
) -> Result<CliResult, sqlx::Error> {
    let up_to_seq: Option<i64> = sqlx::query_scalar(
        "UPDATE collab_run_deliveries
         SET eligible_reason = CASE
                 WHEN eligible_reason = 'action' THEN 'action'
                 ELSE 'ack'
             END,
             eligible_at = COALESCE(
                 eligible_at,
                 CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai'
             )
         WHERE run_id = $1 AND room_id = $2
         RETURNING up_to_seq",
    )
    .bind(run_id)
    .bind(room_id)
    .fetch_optional(&mut **transaction)
    .await?;
    let Some(up_to_seq) = up_to_seq else {
        return Ok(cli_error(4, "NOT_FOUND: room is not in the active run"));
    };
    Ok(CliResult {
        text: "Acknowledged".to_string(),
        exit_code: 0,
        side_effects: vec![CliSideEffect::InboxAcknowledged {
            room_id: room_id.to_string(),
            up_to_seq,
        }],
    })
}
