use sqlx::{Postgres, Transaction};

use crate::protocol::{CliResult, CliSideEffect};
use crate::server::auth::AgentClaims;

use super::{cli_error, shared::mark_delivery_action};

pub(super) async fn set_active(
    transaction: &mut Transaction<'_, Postgres>,
    run_id: &str,
    claims: &AgentClaims,
    message_id: &str,
    emoji: &str,
) -> Result<CliResult, sqlx::Error> {
    let room_id: Option<String> = sqlx::query_scalar(
        "SELECT m.room_id
         FROM collab_messages m
         JOIN collab_room_members rm
           ON rm.room_id = m.room_id AND rm.participant_id = $2
         WHERE m.id = $1",
    )
    .bind(message_id)
    .bind(&claims.sub)
    .fetch_optional(&mut **transaction)
    .await?;
    let Some(room_id) = room_id else {
        return Ok(cli_error(4, "NOT_FOUND: message is not visible"));
    };
    sqlx::query(
        "INSERT INTO collab_reactions (message_id, actor_id, emoji)
         VALUES ($1, $2, $3)
         ON CONFLICT (message_id, actor_id, emoji) DO NOTHING",
    )
    .bind(message_id)
    .bind(&claims.sub)
    .bind(emoji)
    .execute(&mut **transaction)
    .await?;
    mark_delivery_action(transaction, run_id, &room_id).await?;
    Ok(CliResult {
        text: "Reaction is active".to_string(),
        exit_code: 0,
        side_effects: vec![CliSideEffect::ReactionChanged {
            message_id: message_id.to_string(),
            emoji: emoji.to_string(),
            active: true,
        }],
    })
}
