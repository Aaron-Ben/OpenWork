use std::collections::BTreeSet;

use sqlx::{Postgres, Transaction};
use uuid::Uuid;

use crate::protocol::{CliResult, CliSideEffect};
use crate::server::auth::AgentClaims;

use super::{
    cli_error,
    shared::{insert_message, mark_delivery_action, valid_message_body},
};

pub(super) struct Create {
    members: Vec<String>,
    opening: String,
}

pub(super) fn parse_create(argv: &[String]) -> Result<Create, &'static str> {
    let Some(separator) = argv.iter().position(|argument| argument == "--") else {
        return Err("INVALID_ARGUMENT: group create requires -- <opening>");
    };
    if separator < 4 || argv.len() != separator + 2 || !valid_message_body(&argv[separator + 1]) {
        return Err("INVALID_ARGUMENT: group create requires members and one opening body");
    }
    let mut members = BTreeSet::new();
    let mut index = 2;
    while index < separator {
        if argv.get(index).map(String::as_str) != Some("--member") {
            return Err("INVALID_ARGUMENT: expected --member <participant-id>");
        }
        let Some(member) = argv.get(index + 1).filter(|member| !member.is_empty()) else {
            return Err("INVALID_ARGUMENT: missing participant id");
        };
        members.insert(member.clone());
        index += 2;
    }
    if members.is_empty() {
        return Err("INVALID_ARGUMENT: group needs at least one invited member");
    }
    Ok(Create {
        members: members.into_iter().collect(),
        opening: argv[separator + 1].clone(),
    })
}

pub(super) async fn create(
    transaction: &mut Transaction<'_, Postgres>,
    run_id: &str,
    claims: &AgentClaims,
    command: Create,
) -> Result<CliResult, sqlx::Error> {
    let mut member_ids: BTreeSet<String> = command.members.into_iter().collect();
    member_ids.insert(claims.sub.clone());
    let member_ids: Vec<String> = member_ids.into_iter().collect();
    let names: Vec<String> = sqlx::query_scalar(
        "SELECT display_name FROM collab_participants
         WHERE id = ANY($1) ORDER BY display_name",
    )
    .bind(&member_ids)
    .fetch_all(&mut **transaction)
    .await?;
    if names.len() != member_ids.len() {
        return Ok(cli_error(
            4,
            "NOT_FOUND: one or more participants do not exist",
        ));
    }
    let room_id = format!("room_{}", Uuid::new_v4().simple());
    let title = names.join(", ");
    sqlx::query("INSERT INTO collab_rooms (id, kind, title) VALUES ($1, 'group', $2)")
        .bind(&room_id)
        .bind(title)
        .execute(&mut **transaction)
        .await?;
    for member_id in &member_ids {
        sqlx::query(
            "INSERT INTO collab_room_members (room_id, participant_id, last_read_seq)
             VALUES ($1, $2, 0)",
        )
        .bind(&room_id)
        .bind(member_id)
        .execute(&mut **transaction)
        .await?;
    }
    let message_id = format!("msg_{}", Uuid::new_v4().simple());
    let sequence = insert_message(
        transaction,
        &room_id,
        &claims.sub,
        "normal",
        &command.opening,
        Some(&message_id),
    )
    .await?;
    mark_delivery_action(transaction, run_id, &room_id).await?;
    Ok(CliResult {
        text: format!("Created group {room_id}"),
        exit_code: 0,
        side_effects: vec![
            CliSideEffect::GroupRoomCreated {
                room_id: room_id.clone(),
            },
            CliSideEffect::MessagePublished {
                room_id,
                message_id,
                sequence,
            },
        ],
    })
}
