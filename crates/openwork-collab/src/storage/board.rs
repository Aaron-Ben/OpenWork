//! PostgreSQL storage for shared boards and atomic card claims.

use std::time::Duration;

use serde_json::{Value, json};
use sqlx::{Postgres, Transaction};
use time::PrimitiveDateTime;
use uuid::Uuid;

use super::{CollabStorage, MessageRow, StorageError};
use crate::{
    model::{
        Board, BoardColumn, Card, CardClaimOutcome, CardInput, CardMutation, ClaimReleaseCandidate,
        Message, ReleasedClaims,
    },
    time::{china_now, format_china},
};

impl CollabStorage {
    pub async fn create_board(
        &self,
        id: &str,
        room_id: &str,
        title: &str,
    ) -> Result<Board, StorageError> {
        require_text("board id", id)?;
        require_text("board title", title)?;
        sqlx::query("INSERT INTO collab_boards (id, room_id, title) VALUES ($1, $2, $3)")
            .bind(id)
            .bind(room_id)
            .bind(title.trim())
            .execute(&self.pool)
            .await?;
        self.boards(room_id)
            .await?
            .into_iter()
            .find(|board| board.id == id)
            .ok_or_else(|| StorageError::NotFound(format!("board {id}")))
    }

    pub async fn create_board_column(
        &self,
        id: &str,
        board_id: &str,
        title: &str,
        position: i32,
        is_done: bool,
    ) -> Result<BoardColumn, StorageError> {
        require_text("column id", id)?;
        require_text("column title", title)?;
        let row = sqlx::query_as::<_, ColumnRow>(
            "INSERT INTO collab_board_columns (id, board_id, title, position, is_done)
             VALUES ($1, $2, $3, $4, $5)
             RETURNING id, board_id, title, position, is_done",
        )
        .bind(id)
        .bind(board_id)
        .bind(title.trim())
        .bind(position)
        .bind(is_done)
        .fetch_one(&self.pool)
        .await?;
        Ok(row.into_column(Vec::new()))
    }

    pub async fn boards(&self, room_id: &str) -> Result<Vec<Board>, StorageError> {
        let board_rows = sqlx::query_as::<_, BoardRow>(
            "SELECT id, room_id, title, created_at, updated_at
               FROM collab_boards WHERE room_id = $1
              ORDER BY created_at, id",
        )
        .bind(room_id)
        .fetch_all(&self.pool)
        .await?;
        let mut boards = Vec::with_capacity(board_rows.len());
        for board in board_rows {
            let column_rows = sqlx::query_as::<_, ColumnRow>(
                "SELECT id, board_id, title, position, is_done
                   FROM collab_board_columns WHERE board_id = $1
                  ORDER BY position, id",
            )
            .bind(&board.id)
            .fetch_all(&self.pool)
            .await?;
            let mut columns = Vec::with_capacity(column_rows.len());
            for column in column_rows {
                let cards = sqlx::query_as::<_, CardRow>(
                    "SELECT id, board_id, column_id, title, description, position,
                            assignee_id, claimed_by, claimed_at, created_at, updated_at
                       FROM collab_cards WHERE column_id = $1
                      ORDER BY position, created_at, id",
                )
                .bind(&column.id)
                .fetch_all(&self.pool)
                .await?
                .into_iter()
                .map(CardRow::try_into_card)
                .collect::<Result<Vec<_>, _>>()?;
                columns.push(column.into_column(cards));
            }
            boards.push(board.try_into_board(columns)?);
        }
        Ok(boards)
    }

    pub async fn create_card(
        &self,
        input: CardInput,
        actor_id: &str,
    ) -> Result<CardMutation, StorageError> {
        require_text("card title", &input.title)?;
        let mut transaction = self.pool.begin().await?;
        let room_id = card_write_room(
            &mut transaction,
            &input.board_id,
            &input.column_id,
            actor_id,
            input.assignee_id.as_deref(),
        )
        .await?;
        let id = format!("card_{}", Uuid::new_v4().simple());
        let row = sqlx::query_as::<_, CardRow>(
            "INSERT INTO collab_cards (
                id, board_id, column_id, title, description, position, assignee_id
             ) VALUES ($1, $2, $3, $4, $5, $6, $7)
             RETURNING id, board_id, column_id, title, description, position,
                       assignee_id, claimed_by, claimed_at, created_at, updated_at",
        )
        .bind(&id)
        .bind(&input.board_id)
        .bind(&input.column_id)
        .bind(input.title.trim())
        .bind(input.description.as_deref())
        .bind(input.position)
        .bind(input.assignee_id.as_deref())
        .fetch_one(&mut *transaction)
        .await?;
        let payload = json!({
            "type": "card_created",
            "boardId": input.board_id,
            "columnId": input.column_id,
            "cardId": id,
            "title": input.title.trim(),
            "assigneeId": input.assignee_id,
        });
        let body = input.assignee_id.as_ref().map_or_else(
            || format!("{actor_id} created card “{}”", input.title.trim()),
            |assignee| {
                format!(
                    "{actor_id} created card “{}” and assigned it to {assignee}",
                    input.title.trim()
                )
            },
        );
        let message =
            insert_system_message(&mut transaction, &room_id, actor_id, &body, payload).await?;
        transaction.commit().await?;
        Ok(CardMutation {
            card: row.try_into_card()?,
            message,
        })
    }

    pub async fn claim_card(
        &self,
        card_id: &str,
        agent_id: &str,
    ) -> Result<CardClaimOutcome, StorageError> {
        let mut transaction = self.pool.begin().await?;
        let now = china_now();
        let claimed = sqlx::query_as::<_, CardWithRoomRow>(
            "UPDATE collab_cards c
                SET claimed_by = $1, claimed_at = $2, updated_at = $2
               FROM collab_boards b
              WHERE c.id = $3 AND c.claimed_by IS NULL AND c.board_id = b.id
                AND EXISTS (
                    SELECT 1 FROM collab_room_members rm
                     WHERE rm.room_id = b.room_id AND rm.participant_id = $1
                )
              RETURNING c.id, c.board_id, c.column_id, c.title, c.description,
                        c.position, c.assignee_id, c.claimed_by, c.claimed_at,
                        c.created_at, c.updated_at, b.room_id",
        )
        .bind(agent_id)
        .bind(now)
        .bind(card_id)
        .fetch_optional(&mut *transaction)
        .await?;
        let Some(claimed) = claimed else {
            let existing = sqlx::query_scalar::<_, Option<String>>(
                "SELECT claimed_by FROM collab_cards WHERE id = $1",
            )
            .bind(card_id)
            .fetch_optional(&mut *transaction)
            .await?;
            transaction.rollback().await?;
            return existing.flatten().map_or_else(
                || Err(StorageError::NotFound(format!("claimable card {card_id}"))),
                |claimed_by| {
                    Ok(CardClaimOutcome::AlreadyClaimed {
                        card_id: card_id.to_string(),
                        claimed_by,
                    })
                },
            );
        };
        let payload = json!({
            "type": "card_claimed",
            "boardId": claimed.card.board_id,
            "columnId": claimed.card.column_id,
            "cardId": claimed.card.id,
            "claimedBy": agent_id,
        });
        let message = insert_system_message(
            &mut transaction,
            &claimed.room_id,
            agent_id,
            &format!("{agent_id} claimed card “{}”", claimed.card.title),
            payload,
        )
        .await?;
        transaction.commit().await?;
        Ok(CardClaimOutcome::Claimed(Box::new(CardMutation {
            card: claimed.card.try_into_card()?,
            message,
        })))
    }

    pub async fn move_card(
        &self,
        card_id: &str,
        column_id: &str,
        position: i32,
        actor_id: &str,
    ) -> Result<CardMutation, StorageError> {
        let mut transaction = self.pool.begin().await?;
        let now = china_now();
        let moved = sqlx::query_as::<_, CardWithRoomRow>(
            "UPDATE collab_cards c
                SET column_id = target.id, position = $3, updated_at = $4
               FROM collab_board_columns target, collab_boards b
              WHERE c.id = $1 AND target.id = $2 AND target.board_id = c.board_id
                AND c.board_id = b.id
                AND EXISTS (
                    SELECT 1 FROM collab_room_members rm
                     WHERE rm.room_id = b.room_id AND rm.participant_id = $5
                )
              RETURNING c.id, c.board_id, c.column_id, c.title, c.description,
                        c.position, c.assignee_id, c.claimed_by, c.claimed_at,
                        c.created_at, c.updated_at, b.room_id",
        )
        .bind(card_id)
        .bind(column_id)
        .bind(position)
        .bind(now)
        .bind(actor_id)
        .fetch_optional(&mut *transaction)
        .await?
        .ok_or_else(|| StorageError::NotFound(format!("movable card {card_id}")))?;
        let payload = json!({
            "type": "card_moved",
            "boardId": moved.card.board_id,
            "columnId": moved.card.column_id,
            "cardId": moved.card.id,
            "position": moved.card.position,
        });
        let message = insert_system_message(
            &mut transaction,
            &moved.room_id,
            actor_id,
            &format!("{actor_id} moved card “{}”", moved.card.title),
            payload,
        )
        .await?;
        transaction.commit().await?;
        Ok(CardMutation {
            card: moved.card.try_into_card()?,
            message,
        })
    }

    pub async fn release_card_claim(
        &self,
        card_id: &str,
        claimed_by: &str,
        actor_id: &str,
        reason: &str,
    ) -> Result<Option<CardMutation>, StorageError> {
        let mut transaction = self.pool.begin().await?;
        let released = sqlx::query_as::<_, CardWithRoomRow>(
            "UPDATE collab_cards c
                SET claimed_by = NULL, claimed_at = NULL,
                    updated_at = CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai'
               FROM collab_boards b
              WHERE c.id = $1 AND c.claimed_by = $2 AND c.board_id = b.id
              RETURNING c.id, c.board_id, c.column_id, c.title, c.description,
                        c.position, c.assignee_id, c.claimed_by, c.claimed_at,
                        c.created_at, c.updated_at, b.room_id",
        )
        .bind(card_id)
        .bind(claimed_by)
        .fetch_optional(&mut *transaction)
        .await?;
        let Some(released) = released else {
            transaction.rollback().await?;
            return Ok(None);
        };
        let payload = json!({
            "type": "card_claim_released",
            "boardId": released.card.board_id,
            "columnId": released.card.column_id,
            "cardId": released.card.id,
            "previousClaimant": claimed_by,
            "reason": reason,
        });
        let message = insert_system_message(
            &mut transaction,
            &released.room_id,
            actor_id,
            &format!("claim on card “{}” was released", released.card.title),
            payload,
        )
        .await?;
        transaction.commit().await?;
        Ok(Some(CardMutation {
            card: released.card.try_into_card()?,
            message,
        }))
    }

    pub async fn release_all_claims(&self) -> Result<ReleasedClaims, StorageError> {
        let rows = sqlx::query_as::<_, ReleasedRoomRow>(
            "WITH released AS (
                UPDATE collab_cards
                   SET claimed_by = NULL, claimed_at = NULL,
                       updated_at = CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai'
                 WHERE claimed_by IS NOT NULL
                 RETURNING board_id
             )
             SELECT b.room_id, count(*)::BIGINT AS released_count
               FROM released r JOIN collab_boards b ON b.id = r.board_id
              GROUP BY b.room_id ORDER BY b.room_id",
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(ReleasedClaims {
            count: rows.iter().map(|row| row.released_count as u64).sum(),
            room_ids: rows.into_iter().map(|row| row.room_id).collect(),
        })
    }

    pub async fn claim_release_candidates(
        &self,
        grace: Duration,
    ) -> Result<Vec<ClaimReleaseCandidate>, StorageError> {
        let grace = time::Duration::try_from(grace)
            .map_err(|_| StorageError::InvalidInput("claim grace is too large".to_string()))?;
        let cutoff = china_now() - grace;
        Ok(sqlx::query_as::<_, ClaimCandidateRow>(
            "SELECT c.id AS card_id, b.room_id, c.claimed_by,
                    a.opencode_session_id
               FROM collab_cards c
               JOIN collab_boards b ON b.id = c.board_id
               LEFT JOIN collab_agents a ON a.id = c.claimed_by
              WHERE c.claimed_by IS NOT NULL AND c.claimed_at <= $1
              ORDER BY c.claimed_at, c.id",
        )
        .bind(cutoff)
        .fetch_all(&self.pool)
        .await?
        .into_iter()
        .map(ClaimCandidateRow::into_candidate)
        .collect())
    }
}

async fn card_write_room(
    transaction: &mut Transaction<'_, Postgres>,
    board_id: &str,
    column_id: &str,
    actor_id: &str,
    assignee_id: Option<&str>,
) -> Result<String, StorageError> {
    sqlx::query_scalar(
        "SELECT b.room_id
           FROM collab_boards b
           JOIN collab_board_columns c ON c.board_id = b.id AND c.id = $2
           JOIN collab_room_members actor
             ON actor.room_id = b.room_id AND actor.participant_id = $3
          WHERE b.id = $1
            AND ($4::TEXT IS NULL OR EXISTS (
                SELECT 1 FROM collab_room_members assignee
                 WHERE assignee.room_id = b.room_id AND assignee.participant_id = $4
            ))",
    )
    .bind(board_id)
    .bind(column_id)
    .bind(actor_id)
    .bind(assignee_id)
    .fetch_optional(&mut **transaction)
    .await?
    .ok_or_else(|| StorageError::NotFound(format!("writable board {board_id}")))
}

pub(super) async fn insert_system_message(
    transaction: &mut Transaction<'_, Postgres>,
    room_id: &str,
    author_id: &str,
    body: &str,
    payload: Value,
) -> Result<Message, StorageError> {
    let sequence = sqlx::query_scalar::<_, i64>(
        "UPDATE collab_rooms
            SET next_seq = next_seq + 1,
                updated_at = CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai'
          WHERE id = $1 AND next_seq < 9223372036854775807
          RETURNING next_seq",
    )
    .bind(room_id)
    .fetch_optional(&mut **transaction)
    .await?
    .ok_or_else(|| StorageError::NotFoundOrExhausted(format!("room {room_id}")))?;
    let id = format!("msg_{}", Uuid::new_v4().simple());
    let created_at = china_now();
    let row = sqlx::query_as::<_, MessageRow>(
        "INSERT INTO collab_messages (
            id, room_id, sequence, author_id, kind, body, system_payload, created_at
         ) VALUES ($1, $2, $3, $4, 'system', $5, $6, $7)
         RETURNING id, room_id, sequence, author_id, kind, body, system_payload, created_at",
    )
    .bind(&id)
    .bind(room_id)
    .bind(sequence)
    .bind(author_id)
    .bind(body)
    .bind(payload)
    .bind(created_at)
    .fetch_one(&mut **transaction)
    .await?;
    sqlx::query("UPDATE collab_rooms SET last_message_at = $2, updated_at = $2 WHERE id = $1")
        .bind(room_id)
        .bind(created_at)
        .execute(&mut **transaction)
        .await?;
    row.try_into_message()
}

fn require_text(field: &str, value: &str) -> Result<(), StorageError> {
    if value.trim().is_empty() {
        return Err(StorageError::InvalidInput(format!(
            "{field} must not be blank"
        )));
    }
    Ok(())
}

#[derive(Debug, sqlx::FromRow)]
struct BoardRow {
    id: String,
    room_id: String,
    title: String,
    created_at: PrimitiveDateTime,
    updated_at: PrimitiveDateTime,
}

impl BoardRow {
    fn try_into_board(self, columns: Vec<BoardColumn>) -> Result<Board, StorageError> {
        Ok(Board {
            id: self.id,
            room_id: self.room_id,
            title: self.title,
            columns,
            created_at: format_china(self.created_at)?,
            updated_at: format_china(self.updated_at)?,
        })
    }
}

#[derive(Debug, sqlx::FromRow)]
struct ColumnRow {
    id: String,
    board_id: String,
    title: String,
    position: i32,
    is_done: bool,
}

impl ColumnRow {
    fn into_column(self, cards: Vec<Card>) -> BoardColumn {
        BoardColumn {
            id: self.id,
            board_id: self.board_id,
            title: self.title,
            position: self.position,
            is_done: self.is_done,
            cards,
        }
    }
}

#[derive(Debug, sqlx::FromRow)]
struct CardRow {
    id: String,
    board_id: String,
    column_id: String,
    title: String,
    description: Option<String>,
    position: i32,
    assignee_id: Option<String>,
    claimed_by: Option<String>,
    claimed_at: Option<PrimitiveDateTime>,
    created_at: PrimitiveDateTime,
    updated_at: PrimitiveDateTime,
}

impl CardRow {
    fn try_into_card(self) -> Result<Card, StorageError> {
        Ok(Card {
            id: self.id,
            board_id: self.board_id,
            column_id: self.column_id,
            title: self.title,
            description: self.description,
            position: self.position,
            assignee_id: self.assignee_id,
            claimed_by: self.claimed_by,
            claimed_at: self.claimed_at.map(format_china).transpose()?,
            created_at: format_china(self.created_at)?,
            updated_at: format_china(self.updated_at)?,
        })
    }
}

#[derive(Debug, sqlx::FromRow)]
struct CardWithRoomRow {
    #[sqlx(flatten)]
    card: CardRow,
    room_id: String,
}

#[derive(Debug, sqlx::FromRow)]
struct ReleasedRoomRow {
    room_id: String,
    released_count: i64,
}

#[derive(Debug, sqlx::FromRow)]
struct ClaimCandidateRow {
    card_id: String,
    room_id: String,
    claimed_by: String,
    opencode_session_id: Option<String>,
}

impl ClaimCandidateRow {
    fn into_candidate(self) -> ClaimReleaseCandidate {
        ClaimReleaseCandidate {
            card_id: self.card_id,
            room_id: self.room_id,
            claimed_by: self.claimed_by,
            opencode_session_id: self.opencode_session_id,
        }
    }
}
