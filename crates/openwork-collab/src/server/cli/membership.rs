use sqlx::{Postgres, Transaction};
use uuid::Uuid;

use crate::protocol::{CliResult, CliSideEffect};
use crate::server::auth::AgentClaims;

use super::{
    cli_error,
    shared::{insert_message, mark_delivery_action},
};

#[derive(Clone, Copy)]
pub(super) enum Change {
    Invite,
    Leave,
    Kick,
}

pub(super) async fn apply(
    transaction: &mut Transaction<'_, Postgres>,
    run_id: &str,
    claims: &AgentClaims,
    room_id: &str,
    participant_id: &str,
    change: Change,
) -> Result<CliResult, sqlx::Error> {
    if matches!(change, Change::Kick) && participant_id == claims.sub {
        return Ok(cli_error(
            2,
            "INVALID_ARGUMENT: use group leave to remove yourself",
        ));
    }
    let current_sequence: Option<i64> = sqlx::query_scalar(
        "SELECT r.next_seq
         FROM collab_rooms r
         JOIN collab_room_members actor
           ON actor.room_id = r.id AND actor.participant_id = $2
         WHERE r.id = $1 AND r.kind = 'group'
         FOR UPDATE OF r",
    )
    .bind(room_id)
    .bind(&claims.sub)
    .fetch_optional(&mut **transaction)
    .await?;
    let Some(current_sequence) = current_sequence else {
        return Ok(cli_error(4, "NOT_FOUND: group is not visible"));
    };
    let participant_exists: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM collab_participants WHERE id = $1)")
            .bind(participant_id)
            .fetch_one(&mut **transaction)
            .await?;
    if !participant_exists {
        return Ok(cli_error(4, "NOT_FOUND: participant does not exist"));
    }
    let is_member: bool = sqlx::query_scalar(
        "SELECT EXISTS(
            SELECT 1 FROM collab_room_members
            WHERE room_id = $1 AND participant_id = $2
         )",
    )
    .bind(room_id)
    .bind(participant_id)
    .fetch_one(&mut **transaction)
    .await?;
    let (body, change_name) = match change {
        Change::Invite => {
            if is_member {
                return Ok(CliResult {
                    text: "Participant is already a member".to_string(),
                    exit_code: 0,
                    side_effects: Vec::new(),
                });
            }
            sqlx::query(
                "INSERT INTO collab_room_members (room_id, participant_id, last_read_seq)
                 VALUES ($1, $2, $3)",
            )
            .bind(room_id)
            .bind(participant_id)
            .bind(current_sequence)
            .execute(&mut **transaction)
            .await?;
            (
                format!("{} invited {participant_id}", claims.sub),
                "invited",
            )
        }
        Change::Leave => {
            if !is_member {
                return Ok(CliResult {
                    text: "Participant already left".to_string(),
                    exit_code: 0,
                    side_effects: Vec::new(),
                });
            }
            (format!("{} left", claims.sub), "left")
        }
        Change::Kick => {
            if !is_member {
                return Ok(CliResult {
                    text: "Participant is already absent".to_string(),
                    exit_code: 0,
                    side_effects: Vec::new(),
                });
            }
            (format!("{} kicked {participant_id}", claims.sub), "kicked")
        }
    };
    let message_id = format!("msg_{}", Uuid::new_v4().simple());
    let sequence = insert_message(
        transaction,
        room_id,
        &claims.sub,
        "system",
        &body,
        Some(&message_id),
    )
    .await?;
    if !matches!(change, Change::Invite) {
        sqlx::query("DELETE FROM collab_room_members WHERE room_id = $1 AND participant_id = $2")
            .bind(room_id)
            .bind(participant_id)
            .execute(&mut **transaction)
            .await?;
    }
    mark_delivery_action(transaction, run_id, room_id).await?;
    Ok(CliResult {
        text: format!("Membership {change_name}"),
        exit_code: 0,
        side_effects: vec![
            CliSideEffect::MessagePublished {
                room_id: room_id.to_string(),
                message_id,
                sequence,
            },
            CliSideEffect::MembershipChanged {
                room_id: room_id.to_string(),
                participant_id: participant_id.to_string(),
                change: change_name.to_string(),
            },
        ],
    })
}
