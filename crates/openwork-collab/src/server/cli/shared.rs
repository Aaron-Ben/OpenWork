use sqlx::{Postgres, Transaction};
use uuid::Uuid;

use crate::protocol::{CLI_MESSAGE_BODY_MAX_BYTES, CliResult};

const MAX_EMOJI_BYTES: usize = 64;

pub(super) async fn insert_message(
    transaction: &mut Transaction<'_, Postgres>,
    room_id: &str,
    author_id: &str,
    kind: &str,
    body: &str,
    message_id: Option<&str>,
) -> Result<i64, sqlx::Error> {
    let sequence: i64 = sqlx::query_scalar(
        "UPDATE collab_rooms
         SET next_seq = next_seq + 1,
             last_message_at = CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai',
             updated_at = CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai'
         WHERE id = $1 RETURNING next_seq",
    )
    .bind(room_id)
    .fetch_one(&mut **transaction)
    .await?;
    let generated;
    let message_id = match message_id {
        Some(message_id) => message_id,
        None => {
            generated = format!("msg_{}", Uuid::new_v4().simple());
            &generated
        }
    };
    sqlx::query(
        "INSERT INTO collab_messages (id, room_id, sequence, author_id, kind, body)
         VALUES ($1, $2, $3, $4, $5, $6)",
    )
    .bind(message_id)
    .bind(room_id)
    .bind(sequence)
    .bind(author_id)
    .bind(kind)
    .bind(body)
    .execute(&mut **transaction)
    .await?;
    Ok(sequence)
}

pub(super) async fn mark_delivery_action(
    transaction: &mut Transaction<'_, Postgres>,
    run_id: &str,
    room_id: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE collab_run_deliveries
         SET eligible_reason = 'action',
             eligible_at = CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai'
         WHERE run_id = $1 AND room_id = $2",
    )
    .bind(run_id)
    .bind(room_id)
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

pub(super) fn valid_message_body(body: &str) -> bool {
    !body.trim().is_empty()
        && body.len() <= CLI_MESSAGE_BODY_MAX_BYTES
        && !body.as_bytes().contains(&0)
}

pub(super) fn valid_emoji(emoji: &str) -> bool {
    !emoji.trim().is_empty() && emoji.len() <= MAX_EMOJI_BYTES && !emoji.as_bytes().contains(&0)
}

pub(super) fn error(exit_code: i32, message: &str) -> CliResult {
    CliResult {
        text: message.to_string(),
        exit_code,
        side_effects: Vec::new(),
    }
}
