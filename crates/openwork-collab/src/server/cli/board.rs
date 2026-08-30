use std::fmt::Write as _;

use sqlx::{Postgres, Transaction};
use uuid::Uuid;

use crate::{
    protocol::{CliResult, CliSideEffect},
    server::auth::AgentClaims,
};

use super::{
    cli_error,
    shared::{insert_message, mark_delivery_action},
};

#[derive(sqlx::FromRow)]
struct CardListRow {
    column_id: String,
    column_title: String,
    is_done: bool,
    card_id: Option<String>,
    card_title: Option<String>,
    assignee_id: Option<String>,
    claimed_by: Option<String>,
}

pub(super) struct CreateCard<'a> {
    board_id: &'a str,
    column_id: &'a str,
    title: &'a str,
    assignee_id: Option<&'a str>,
}

pub(super) struct MoveCard<'a> {
    card_id: &'a str,
    column_id: &'a str,
    position: i32,
}

pub(super) fn parse_create(argv: &[String]) -> Result<CreateCard<'_>, &'static str> {
    if argv.len() < 8
        || argv.first().map(String::as_str) != Some("card")
        || argv.get(1).map(String::as_str) != Some("create")
    {
        return Err(
            "INVALID_ARGUMENT: expected card create --board <id> --column <id> --title <text> [--assignee <id>]",
        );
    }
    let mut board_id = None;
    let mut column_id = None;
    let mut title = None;
    let mut assignee_id = None;
    let mut index = 2;
    while index < argv.len() {
        let value = argv
            .get(index + 1)
            .ok_or("INVALID_ARGUMENT: every card create flag requires one value")?;
        match argv[index].as_str() {
            "--board" if board_id.replace(value.as_str()).is_none() => {}
            "--column" if column_id.replace(value.as_str()).is_none() => {}
            "--title" if title.replace(value.as_str()).is_none() => {}
            "--assignee" if assignee_id.replace(value.as_str()).is_none() => {}
            _ => return Err("INVALID_ARGUMENT: duplicate or unknown card create flag"),
        }
        index += 2;
    }
    let (Some(board_id), Some(column_id), Some(title)) = (board_id, column_id, title) else {
        return Err("INVALID_ARGUMENT: card create requires --board, --column, and --title");
    };
    if title.trim().is_empty() || title.len() > 500 {
        return Err("INVALID_ARGUMENT: card title must be 1..500 bytes");
    }
    Ok(CreateCard {
        board_id,
        column_id,
        title: title.trim(),
        assignee_id,
    })
}

pub(super) fn parse_move(argv: &[String]) -> Result<MoveCard<'_>, &'static str> {
    let [
        card,
        move_command,
        card_id,
        column_flag,
        column_id,
        position_flag,
        position,
    ] = argv
    else {
        return Err("INVALID_ARGUMENT: expected card move <card-id> --column <id> --position <n>");
    };
    if card != "card"
        || move_command != "move"
        || column_flag != "--column"
        || position_flag != "--position"
    {
        return Err("INVALID_ARGUMENT: expected card move <card-id> --column <id> --position <n>");
    }
    let position = position
        .parse::<i32>()
        .ok()
        .filter(|position| *position >= 0)
        .ok_or("INVALID_ARGUMENT: card position must be a non-negative integer")?;
    Ok(MoveCard {
        card_id,
        column_id,
        position,
    })
}

pub(super) async fn list(
    transaction: &mut Transaction<'_, Postgres>,
    claims: &AgentClaims,
    room_id: &str,
) -> Result<CliResult, sqlx::Error> {
    let boards: Vec<(String, String)> = sqlx::query_as(
        "SELECT b.id, b.title
         FROM collab_boards b
         JOIN collab_room_members member
           ON member.room_id = b.room_id AND member.participant_id = $2
         WHERE b.room_id = $1 ORDER BY b.created_at, b.id",
    )
    .bind(room_id)
    .bind(&claims.sub)
    .fetch_all(&mut **transaction)
    .await?;
    let mut text = format!("Boards for room {room_id}:\n");
    if boards.is_empty() {
        text.push_str("No boards.");
    }
    for (board_id, board_title) in boards {
        let _ = writeln!(text, "board {board_id}: {board_title}");
        let rows: Vec<CardListRow> = sqlx::query_as(
            "SELECT board_column.id AS column_id,
                        board_column.title AS column_title,
                        board_column.is_done,
                        card.id AS card_id,
                        card.title AS card_title,
                        card.assignee_id,
                        card.claimed_by
                 FROM collab_board_columns board_column
                 LEFT JOIN collab_cards card ON card.column_id = board_column.id
                 WHERE board_column.board_id = $1
                 ORDER BY board_column.position, card.position, card.id",
        )
        .bind(&board_id)
        .fetch_all(&mut **transaction)
        .await?;
        let mut previous = String::new();
        for row in rows {
            if previous != row.column_id {
                let _ = writeln!(
                    text,
                    "  column {}: {} done={}",
                    row.column_id, row.column_title, row.is_done
                );
                previous = row.column_id;
            }
            if let (Some(card_id), Some(card_title)) = (row.card_id, row.card_title) {
                let _ = writeln!(
                    text,
                    "    card {card_id}: {card_title} assignee={} claimed_by={}",
                    row.assignee_id.as_deref().unwrap_or("none"),
                    row.claimed_by.as_deref().unwrap_or("none")
                );
            }
        }
    }
    Ok(CliResult {
        text,
        exit_code: 0,
        side_effects: Vec::new(),
    })
}

pub(super) async fn create(
    transaction: &mut Transaction<'_, Postgres>,
    run_id: &str,
    claims: &AgentClaims,
    command: CreateCard<'_>,
) -> Result<CliResult, sqlx::Error> {
    let room_id: Option<String> = sqlx::query_scalar(
        "SELECT board.room_id
         FROM collab_boards board
         JOIN collab_board_columns board_column
           ON board_column.board_id = board.id AND board_column.id = $2
         JOIN collab_room_members member
           ON member.room_id = board.room_id AND member.participant_id = $3
         WHERE board.id = $1
         FOR UPDATE OF board",
    )
    .bind(command.board_id)
    .bind(command.column_id)
    .bind(&claims.sub)
    .fetch_optional(&mut **transaction)
    .await?;
    let Some(room_id) = room_id else {
        return Ok(cli_error(4, "NOT_FOUND: board or column is not visible"));
    };
    if let Some(assignee_id) = command.assignee_id {
        let member: bool = sqlx::query_scalar(
            "SELECT EXISTS(
                SELECT 1 FROM collab_room_members member
                JOIN collab_agents agent ON agent.id = member.participant_id
                WHERE member.room_id = $1 AND member.participant_id = $2 AND agent.enabled
             )",
        )
        .bind(&room_id)
        .bind(assignee_id)
        .fetch_one(&mut **transaction)
        .await?;
        if !member {
            return Ok(cli_error(
                4,
                "NOT_FOUND: assignee is not an enabled Agent in the board room",
            ));
        }
    }
    let position: i32 =
        sqlx::query_scalar("SELECT COUNT(*)::INTEGER FROM collab_cards WHERE column_id = $1")
            .bind(command.column_id)
            .fetch_one(&mut **transaction)
            .await?;
    let card_id = format!("card_{}", Uuid::new_v4().simple());
    sqlx::query(
        "INSERT INTO collab_cards (
            id, board_id, column_id, title, position, assignee_id
         ) VALUES ($1, $2, $3, $4, $5, $6)",
    )
    .bind(&card_id)
    .bind(command.board_id)
    .bind(command.column_id)
    .bind(command.title)
    .bind(position)
    .bind(command.assignee_id)
    .execute(&mut **transaction)
    .await?;
    let (message_id, sequence) = system_message(
        transaction,
        &room_id,
        &claims.sub,
        "card.created",
        &format!("{} created card {}: {}", claims.sub, card_id, command.title),
        serde_json::json!({
            "boardId": command.board_id,
            "cardId": card_id,
            "columnId": command.column_id,
            "assigneeId": command.assignee_id,
        }),
    )
    .await?;
    mark_delivery_action(transaction, run_id, &room_id).await?;
    Ok(CliResult {
        text: format!("Created card {card_id}"),
        exit_code: 0,
        side_effects: vec![
            CliSideEffect::CardCreated {
                board_id: command.board_id.to_string(),
                card_id,
            },
            CliSideEffect::MessagePublished {
                room_id,
                message_id,
                sequence,
            },
        ],
    })
}

pub(super) async fn claim(
    transaction: &mut Transaction<'_, Postgres>,
    run_id: &str,
    claims: &AgentClaims,
    card_id: &str,
) -> Result<CliResult, sqlx::Error> {
    let row: Option<(String, Option<String>)> = sqlx::query_as(
        "SELECT board.room_id, card.claimed_by
         FROM collab_cards card
         JOIN collab_boards board ON board.id = card.board_id
         JOIN collab_board_columns board_column
           ON board_column.id = card.column_id AND NOT board_column.is_done
         JOIN collab_room_members member
           ON member.room_id = board.room_id AND member.participant_id = $2
         WHERE card.id = $1 FOR UPDATE OF card",
    )
    .bind(card_id)
    .bind(&claims.sub)
    .fetch_optional(&mut **transaction)
    .await?;
    let Some((room_id, claimed_by)) = row else {
        return Ok(cli_error(4, "NOT_FOUND: open card is not visible"));
    };
    if let Some(claimed_by) = claimed_by {
        return if claimed_by == claims.sub {
            Ok(CliResult {
                text: format!("Card {card_id} is already claimed by this Agent"),
                exit_code: 0,
                side_effects: Vec::new(),
            })
        } else {
            Ok(cli_error(5, "CONFLICT: card is already claimed"))
        };
    }
    sqlx::query(
        "UPDATE collab_cards
         SET claimed_by = $2, claimed_at = CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai',
             updated_at = CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai'
         WHERE id = $1",
    )
    .bind(card_id)
    .bind(&claims.sub)
    .execute(&mut **transaction)
    .await?;
    let (message_id, sequence) = system_message(
        transaction,
        &room_id,
        &claims.sub,
        "card.claimed",
        &format!("{} claimed card {}", claims.sub, card_id),
        serde_json::json!({"cardId": card_id, "claimedBy": claims.sub}),
    )
    .await?;
    mark_delivery_action(transaction, run_id, &room_id).await?;
    Ok(CliResult {
        text: format!("Claimed card {card_id}"),
        exit_code: 0,
        side_effects: vec![
            CliSideEffect::CardClaimed {
                card_id: card_id.to_string(),
                claimed_by: claims.sub.clone(),
            },
            CliSideEffect::MessagePublished {
                room_id,
                message_id,
                sequence,
            },
        ],
    })
}

pub(super) async fn move_card(
    transaction: &mut Transaction<'_, Postgres>,
    run_id: &str,
    claims: &AgentClaims,
    command: MoveCard<'_>,
) -> Result<CliResult, sqlx::Error> {
    let card: Option<(String, String, String, i32)> = sqlx::query_as(
        "SELECT board.room_id, card.board_id, card.column_id, card.position
         FROM collab_cards card
         JOIN collab_boards board ON board.id = card.board_id
         JOIN collab_room_members member
           ON member.room_id = board.room_id AND member.participant_id = $2
         WHERE card.id = $1 FOR UPDATE OF card, board",
    )
    .bind(command.card_id)
    .bind(&claims.sub)
    .fetch_optional(&mut **transaction)
    .await?;
    let Some((room_id, board_id, old_column, old_position)) = card else {
        return Ok(cli_error(4, "NOT_FOUND: card is not visible"));
    };
    let target_exists: bool = sqlx::query_scalar(
        "SELECT EXISTS(
            SELECT 1 FROM collab_board_columns WHERE id = $1 AND board_id = $2
         )",
    )
    .bind(command.column_id)
    .bind(&board_id)
    .fetch_one(&mut **transaction)
    .await?;
    if !target_exists {
        return Ok(cli_error(
            4,
            "NOT_FOUND: target column is not in the card board",
        ));
    }
    let target_count: i32 = sqlx::query_scalar(
        "SELECT COUNT(*)::INTEGER FROM collab_cards WHERE column_id = $1 AND id <> $2",
    )
    .bind(command.column_id)
    .bind(command.card_id)
    .fetch_one(&mut **transaction)
    .await?;
    let position = command.position.min(target_count);
    if old_column == command.column_id {
        if position < old_position {
            sqlx::query(
                "UPDATE collab_cards SET position = position + 1
                 WHERE column_id = $1 AND id <> $2 AND position >= $3 AND position < $4",
            )
            .bind(&old_column)
            .bind(command.card_id)
            .bind(position)
            .bind(old_position)
            .execute(&mut **transaction)
            .await?;
        } else if position > old_position {
            sqlx::query(
                "UPDATE collab_cards SET position = position - 1
                 WHERE column_id = $1 AND id <> $2 AND position > $3 AND position <= $4",
            )
            .bind(&old_column)
            .bind(command.card_id)
            .bind(old_position)
            .bind(position)
            .execute(&mut **transaction)
            .await?;
        }
    } else {
        sqlx::query(
            "UPDATE collab_cards SET position = position - 1
             WHERE column_id = $1 AND position > $2",
        )
        .bind(&old_column)
        .bind(old_position)
        .execute(&mut **transaction)
        .await?;
        sqlx::query(
            "UPDATE collab_cards SET position = position + 1
             WHERE column_id = $1 AND position >= $2",
        )
        .bind(command.column_id)
        .bind(position)
        .execute(&mut **transaction)
        .await?;
    }
    sqlx::query(
        "UPDATE collab_cards
         SET column_id = $2, position = $3,
             updated_at = CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai'
         WHERE id = $1",
    )
    .bind(command.card_id)
    .bind(command.column_id)
    .bind(position)
    .execute(&mut **transaction)
    .await?;
    let (message_id, sequence) = system_message(
        transaction,
        &room_id,
        &claims.sub,
        "card.moved",
        &format!("{} moved card {}", claims.sub, command.card_id),
        serde_json::json!({
            "cardId": command.card_id,
            "columnId": command.column_id,
            "position": position,
        }),
    )
    .await?;
    mark_delivery_action(transaction, run_id, &room_id).await?;
    Ok(CliResult {
        text: format!("Moved card {}", command.card_id),
        exit_code: 0,
        side_effects: vec![
            CliSideEffect::CardMoved {
                card_id: command.card_id.to_string(),
                column_id: command.column_id.to_string(),
                position,
            },
            CliSideEffect::MessagePublished {
                room_id,
                message_id,
                sequence,
            },
        ],
    })
}

async fn system_message(
    transaction: &mut Transaction<'_, Postgres>,
    room_id: &str,
    author_id: &str,
    event_type: &str,
    body: &str,
    payload: serde_json::Value,
) -> Result<(String, i64), sqlx::Error> {
    let message_id = format!("msg_{}", Uuid::new_v4().simple());
    let sequence = insert_message(
        transaction,
        room_id,
        author_id,
        "system",
        body,
        Some(&message_id),
    )
    .await?;
    sqlx::query(
        "UPDATE collab_messages
         SET system_payload = jsonb_build_object('event', $2, 'data', $3::jsonb)
         WHERE id = $1",
    )
    .bind(&message_id)
    .bind(event_type)
    .bind(payload)
    .execute(&mut **transaction)
    .await?;
    Ok((message_id, sequence))
}
