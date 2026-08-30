use sqlx::{Postgres, Transaction};
use uuid::Uuid;

use crate::protocol::{CliResult, CliSideEffect};
use crate::server::{auth::AgentClaims, rooms::get_or_create_direct_room};

use super::{
    cli_error,
    shared::{insert_message, mark_delivery_action},
};

pub(super) async fn send(
    transaction: &mut Transaction<'_, Postgres>,
    run_id: &str,
    claims: &AgentClaims,
    participant_id: &str,
    body: &str,
) -> Result<CliResult, sqlx::Error> {
    if participant_id == claims.sub {
        return Ok(cli_error(2, "INVALID_ARGUMENT: cannot DM yourself"));
    }
    let participant_exists: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM collab_participants WHERE id = $1)")
            .bind(participant_id)
            .fetch_one(&mut **transaction)
            .await?;
    if !participant_exists {
        return Ok(cli_error(4, "NOT_FOUND: participant does not exist"));
    }
    let (room_id, _) = get_or_create_direct_room(transaction, &claims.sub, participant_id).await?;
    let message_id = format!("msg_{}", Uuid::new_v4().simple());
    let sequence = insert_message(
        transaction,
        &room_id,
        &claims.sub,
        "normal",
        body,
        Some(&message_id),
    )
    .await?;
    mark_delivery_action(transaction, run_id, &room_id).await?;
    Ok(CliResult {
        text: format!("Sent direct message in {room_id}"),
        exit_code: 0,
        side_effects: vec![
            CliSideEffect::DirectRoomOpened {
                room_id: room_id.clone(),
                participant_id: participant_id.to_string(),
            },
            CliSideEffect::MessagePublished {
                room_id,
                message_id,
                sequence,
            },
        ],
    })
}
