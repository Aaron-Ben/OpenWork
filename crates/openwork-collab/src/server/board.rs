use std::collections::BTreeMap;

use sqlx::{FromRow, PgPool, Postgres, Transaction};

use crate::protocol::{BoardColumnView, BoardView, CardView, entity_id};

#[derive(Clone)]
pub(crate) struct Board {
    pool: PgPool,
}

pub(crate) enum BoardOperationError {
    Domain {
        code: &'static str,
        message: &'static str,
    },
    Database(sqlx::Error),
}

impl From<sqlx::Error> for BoardOperationError {
    fn from(error: sqlx::Error) -> Self {
        Self::Database(error)
    }
}

impl Board {
    pub(crate) fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub(crate) async fn create_in(
        transaction: &mut Transaction<'_, Postgres>,
        title: &str,
        description: Option<&str>,
        actor_id: &str,
    ) -> Result<BoardView, sqlx::Error> {
        let title = title.trim();
        if title.is_empty() || title.len() > 200 {
            return Err(protocol_error(
                "INVALID_ARGUMENT: board title must be 1..200 bytes",
            ));
        }
        let board_id = entity_id("board");
        sqlx::query(
            "INSERT INTO collab_boards (id, title, description, created_by)
             VALUES ($1, $2, $3, $4)",
        )
        .bind(&board_id)
        .bind(title)
        .bind(normalize_optional(description))
        .bind(actor_id)
        .execute(&mut **transaction)
        .await?;
        for (position, title, is_terminal) in
            [(0, "Todo", false), (1, "Doing", false), (2, "Done", true)]
        {
            sqlx::query(
                "INSERT INTO collab_board_columns (
                    id, board_id, title, position, is_terminal
                 ) VALUES ($1, $2, $3, $4, $5)",
            )
            .bind(entity_id("col"))
            .bind(&board_id)
            .bind(title)
            .bind(position)
            .bind(is_terminal)
            .execute(&mut **transaction)
            .await?;
        }
        Self::get_in(transaction, &board_id).await
    }

    pub(crate) async fn get_in(
        transaction: &mut Transaction<'_, Postgres>,
        board_id: &str,
    ) -> Result<BoardView, sqlx::Error> {
        let rows = sqlx::query_as::<_, BoardRow>(
            "SELECT board.id AS board_id, board.title AS board_title,
                    board.description AS board_description, board.created_by AS board_created_by,
                    board_column.id AS column_id, board_column.title AS column_title,
                    board_column.position AS column_position, board_column.is_terminal,
                    card.id AS card_id, card.column_id AS card_column_id,
                    card.title AS card_title, card.description AS card_description,
                    card.position AS card_position, card.assignee_id,
                    card.created_by AS card_created_by
             FROM collab_boards board
             LEFT JOIN collab_board_columns board_column ON board_column.board_id = board.id
             LEFT JOIN collab_cards card ON card.column_id = board_column.id
             WHERE board.id = $1
             ORDER BY board_column.position, card.position, card.id",
        )
        .bind(board_id)
        .fetch_all(&mut **transaction)
        .await?;
        assemble(rows)
            .into_iter()
            .next()
            .ok_or(sqlx::Error::RowNotFound)
    }

    pub(crate) async fn update_in(
        transaction: &mut Transaction<'_, Postgres>,
        board_id: &str,
        title: &str,
        description: Option<&str>,
    ) -> Result<BoardView, BoardOperationError> {
        let title = valid_title(title, 200, "board title must be 1..200 bytes")?;
        let updated = sqlx::query(
            "UPDATE collab_boards
             SET title = $2, description = $3,
                 updated_at = CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai'
             WHERE id = $1",
        )
        .bind(board_id)
        .bind(title)
        .bind(normalize_optional(description))
        .execute(&mut **transaction)
        .await?;
        if updated.rows_affected() == 0 {
            return Err(domain("NOT_FOUND", "Board does not exist"));
        }
        Self::get_in(transaction, board_id)
            .await
            .map_err(Into::into)
    }

    pub(crate) async fn delete_in(
        transaction: &mut Transaction<'_, Postgres>,
        board_id: &str,
    ) -> Result<(), BoardOperationError> {
        lock_board(transaction, board_id).await?;
        let has_cards: bool =
            sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM collab_cards WHERE board_id = $1)")
                .bind(board_id)
                .fetch_one(&mut **transaction)
                .await?;
        if has_cards {
            return Err(domain("CONFLICT", "Board still contains Cards"));
        }
        sqlx::query("DELETE FROM collab_boards WHERE id = $1")
            .bind(board_id)
            .execute(&mut **transaction)
            .await?;
        Ok(())
    }

    pub(crate) async fn create_column_in(
        transaction: &mut Transaction<'_, Postgres>,
        board_id: &str,
        title: &str,
        is_terminal: bool,
    ) -> Result<BoardView, BoardOperationError> {
        let title = valid_title(title, 200, "Column title must be 1..200 bytes")?;
        lock_board(transaction, board_id).await?;
        let position: i32 = sqlx::query_scalar(
            "SELECT COUNT(*)::INTEGER FROM collab_board_columns WHERE board_id = $1",
        )
        .bind(board_id)
        .fetch_one(&mut **transaction)
        .await?;
        sqlx::query(
            "INSERT INTO collab_board_columns (id, board_id, title, position, is_terminal)
             VALUES ($1, $2, $3, $4, $5)",
        )
        .bind(entity_id("col"))
        .bind(board_id)
        .bind(title)
        .bind(position)
        .bind(is_terminal)
        .execute(&mut **transaction)
        .await?;
        Self::get_in(transaction, board_id)
            .await
            .map_err(Into::into)
    }

    pub(crate) async fn update_column_in(
        transaction: &mut Transaction<'_, Postgres>,
        column_id: &str,
        title: &str,
        is_terminal: bool,
    ) -> Result<BoardView, BoardOperationError> {
        let title = valid_title(title, 200, "Column title must be 1..200 bytes")?;
        let board_id = board_id_for_column(transaction, column_id).await?;
        lock_board(transaction, &board_id).await?;
        let updated = sqlx::query(
            "UPDATE collab_board_columns
             SET title = $2, is_terminal = $3
             WHERE id = $1 AND board_id = $4",
        )
        .bind(column_id)
        .bind(title)
        .bind(is_terminal)
        .bind(&board_id)
        .execute(&mut **transaction)
        .await?;
        if updated.rows_affected() == 0 {
            return Err(domain("NOT_FOUND", "Column does not exist"));
        }
        touch_board(transaction, &board_id).await?;
        Self::get_in(transaction, &board_id)
            .await
            .map_err(Into::into)
    }

    pub(crate) async fn move_column_in(
        transaction: &mut Transaction<'_, Postgres>,
        column_id: &str,
        before_column_id: Option<&str>,
    ) -> Result<BoardView, BoardOperationError> {
        sqlx::query("SET CONSTRAINTS collab_board_columns_position_unique DEFERRED")
            .execute(&mut **transaction)
            .await?;
        let board_id = board_id_for_column(transaction, column_id).await?;
        lock_board(transaction, &board_id).await?;
        let mut columns = locked_column_ids(transaction, &board_id).await?;
        columns.retain(|id| id != column_id);
        let index = match before_column_id {
            Some(before) => columns
                .iter()
                .position(|id| id == before)
                .ok_or_else(|| domain("NOT_FOUND", "before Column is not in the Board"))?,
            None => columns.len(),
        };
        columns.insert(index, column_id.to_string());
        renumber_columns(transaction, &board_id, &columns).await?;
        touch_board(transaction, &board_id).await?;
        Self::get_in(transaction, &board_id)
            .await
            .map_err(Into::into)
    }

    pub(crate) async fn delete_column_in(
        transaction: &mut Transaction<'_, Postgres>,
        column_id: &str,
    ) -> Result<BoardView, BoardOperationError> {
        sqlx::query("SET CONSTRAINTS collab_board_columns_position_unique DEFERRED")
            .execute(&mut **transaction)
            .await?;
        let board_id = board_id_for_column(transaction, column_id).await?;
        lock_board(transaction, &board_id).await?;
        let column_ids = [column_id.to_string()];
        if lock_columns(transaction, &column_ids).await?.len() != 1 {
            return Err(domain("NOT_FOUND", "Column does not exist"));
        }
        let has_cards: bool =
            sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM collab_cards WHERE column_id = $1)")
                .bind(column_id)
                .fetch_one(&mut **transaction)
                .await?;
        if has_cards {
            return Err(domain("CONFLICT", "Column still contains Cards"));
        }
        sqlx::query("DELETE FROM collab_board_columns WHERE id = $1")
            .bind(column_id)
            .execute(&mut **transaction)
            .await?;
        let columns = locked_column_ids(transaction, &board_id).await?;
        renumber_columns(transaction, &board_id, &columns).await?;
        touch_board(transaction, &board_id).await?;
        Self::get_in(transaction, &board_id)
            .await
            .map_err(Into::into)
    }

    pub(crate) async fn list(&self) -> Result<Vec<BoardView>, sqlx::Error> {
        self.list_filtered(None).await
    }

    pub(crate) async fn list_in(
        transaction: &mut Transaction<'_, Postgres>,
    ) -> Result<Vec<BoardView>, sqlx::Error> {
        let rows = sqlx::query_as::<_, BoardRow>(
            "SELECT board.id AS board_id, board.title AS board_title,
                    board.description AS board_description, board.created_by AS board_created_by,
                    board_column.id AS column_id, board_column.title AS column_title,
                    board_column.position AS column_position, board_column.is_terminal,
                    card.id AS card_id, card.column_id AS card_column_id,
                    card.title AS card_title, card.description AS card_description,
                    card.position AS card_position, card.assignee_id,
                    card.created_by AS card_created_by
             FROM collab_boards board
             LEFT JOIN collab_board_columns board_column ON board_column.board_id = board.id
             LEFT JOIN collab_cards card ON card.column_id = board_column.id
             ORDER BY board.created_at, board.id, board_column.position,
                      card.position, card.id",
        )
        .fetch_all(&mut **transaction)
        .await?;
        Ok(assemble(rows))
    }

    async fn list_filtered(&self, board_id: Option<&str>) -> Result<Vec<BoardView>, sqlx::Error> {
        let rows = sqlx::query_as::<_, BoardRow>(
            "SELECT board.id AS board_id, board.title AS board_title,
                    board.description AS board_description, board.created_by AS board_created_by,
                    board_column.id AS column_id, board_column.title AS column_title,
                    board_column.position AS column_position, board_column.is_terminal,
                    card.id AS card_id, card.column_id AS card_column_id,
                    card.title AS card_title, card.description AS card_description,
                    card.position AS card_position, card.assignee_id,
                    card.created_by AS card_created_by
             FROM collab_boards board
             LEFT JOIN collab_board_columns board_column ON board_column.board_id = board.id
             LEFT JOIN collab_cards card ON card.column_id = board_column.id
             WHERE ($1::TEXT IS NULL OR board.id = $1)
             ORDER BY board.created_at, board.id, board_column.position,
                      card.position, card.id",
        )
        .bind(board_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(assemble(rows))
    }

    pub(crate) async fn list_cards_in(
        transaction: &mut Transaction<'_, Postgres>,
        board_id: Option<&str>,
    ) -> Result<Vec<CardView>, sqlx::Error> {
        sqlx::query_as::<_, CardRow>(
            "SELECT id, board_id, column_id, title, description, position,
                    assignee_id, created_by
             FROM collab_cards
             WHERE ($1::TEXT IS NULL OR board_id = $1)
             ORDER BY board_id, column_id, position, id",
        )
        .bind(board_id)
        .fetch_all(&mut **transaction)
        .await
        .map(|rows| rows.into_iter().map(CardView::from).collect())
    }

    pub(crate) async fn get_card_in(
        transaction: &mut Transaction<'_, Postgres>,
        card_id: &str,
    ) -> Result<CardView, BoardOperationError> {
        card(transaction, card_id)
            .await
            .map_err(|error| match error {
                sqlx::Error::RowNotFound => domain("NOT_FOUND", "Card does not exist"),
                error => error.into(),
            })
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn create_card_in(
        transaction: &mut Transaction<'_, Postgres>,
        board_id: &str,
        column_id: &str,
        title: &str,
        description: Option<&str>,
        assignee_id: Option<&str>,
        actor_id: &str,
    ) -> Result<CardView, BoardOperationError> {
        let title = valid_title(title, 500, "card title must be 1..500 bytes")?;
        lock_board(transaction, board_id).await?;
        let column_exists: Option<String> = sqlx::query_scalar(
            "SELECT id FROM collab_board_columns
             WHERE id = $1 AND board_id = $2 FOR UPDATE",
        )
        .bind(column_id)
        .bind(board_id)
        .fetch_optional(&mut **transaction)
        .await?;
        if column_exists.is_none() {
            return Err(domain("NOT_FOUND", "Board or Column does not exist"));
        }
        if let Some(assignee_id) = assignee_id {
            ensure_active_participant(transaction, assignee_id).await?;
        }
        let position: i32 =
            sqlx::query_scalar("SELECT COUNT(*)::INTEGER FROM collab_cards WHERE column_id = $1")
                .bind(column_id)
                .fetch_one(&mut **transaction)
                .await?;
        let card_id = entity_id("card");
        sqlx::query(
            "INSERT INTO collab_cards (
                id, board_id, column_id, title, description, position,
                assignee_id, created_by
             ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
        )
        .bind(&card_id)
        .bind(board_id)
        .bind(column_id)
        .bind(title)
        .bind(normalize_optional(description))
        .bind(position)
        .bind(assignee_id)
        .bind(actor_id)
        .execute(&mut **transaction)
        .await?;
        card(transaction, &card_id).await.map_err(Into::into)
    }

    pub(crate) async fn update_card_in(
        transaction: &mut Transaction<'_, Postgres>,
        card_id: &str,
        title: &str,
        description: Option<&str>,
    ) -> Result<CardView, BoardOperationError> {
        let title = valid_title(title, 500, "card title must be 1..500 bytes")?;
        let updated = sqlx::query(
            "UPDATE collab_cards
             SET title = $2, description = $3,
                 updated_at = CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai'
             WHERE id = $1",
        )
        .bind(card_id)
        .bind(title)
        .bind(normalize_optional(description))
        .execute(&mut **transaction)
        .await?;
        if updated.rows_affected() == 0 {
            return Err(domain("NOT_FOUND", "Card does not exist"));
        }
        card(transaction, card_id).await.map_err(Into::into)
    }

    pub(crate) async fn assign_card_in(
        transaction: &mut Transaction<'_, Postgres>,
        card_id: &str,
        assignee_id: Option<&str>,
    ) -> Result<CardView, BoardOperationError> {
        if let Some(assignee_id) = assignee_id {
            ensure_active_participant(transaction, assignee_id).await?;
        }
        let updated = sqlx::query(
            "UPDATE collab_cards
             SET assignee_id = $2,
                 updated_at = CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai'
             WHERE id = $1",
        )
        .bind(card_id)
        .bind(assignee_id)
        .execute(&mut **transaction)
        .await?;
        if updated.rows_affected() == 0 {
            return Err(domain("NOT_FOUND", "Card does not exist"));
        }
        card(transaction, card_id).await.map_err(Into::into)
    }

    pub(crate) async fn delete_card_in(
        transaction: &mut Transaction<'_, Postgres>,
        card_id: &str,
    ) -> Result<String, BoardOperationError> {
        sqlx::query("SET CONSTRAINTS collab_cards_position_unique DEFERRED")
            .execute(&mut **transaction)
            .await?;
        let current: Option<(String, String)> =
            sqlx::query_as("SELECT board_id, column_id FROM collab_cards WHERE id = $1")
                .bind(card_id)
                .fetch_optional(&mut **transaction)
                .await?;
        let Some((board_id, _)) = current else {
            return Err(domain("NOT_FOUND", "Card does not exist"));
        };
        lock_board(transaction, &board_id).await?;
        let column_id: Option<String> =
            sqlx::query_scalar("SELECT column_id FROM collab_cards WHERE id = $1")
                .bind(card_id)
                .fetch_optional(&mut **transaction)
                .await?;
        let Some(column_id) = column_id else {
            return Err(domain("NOT_FOUND", "Card does not exist"));
        };
        lock_columns(transaction, std::slice::from_ref(&column_id)).await?;
        let deleted = sqlx::query("DELETE FROM collab_cards WHERE id = $1 AND column_id = $2")
            .bind(card_id)
            .bind(&column_id)
            .execute(&mut **transaction)
            .await?;
        if deleted.rows_affected() == 0 {
            return Err(domain(
                "CONFLICT",
                "Card changed while it was being deleted",
            ));
        }
        let cards = locked_card_ids(transaction, &column_id).await?;
        renumber(transaction, &column_id, &cards).await?;
        Ok(board_id)
    }

    pub(crate) async fn claim_card_in(
        transaction: &mut Transaction<'_, Postgres>,
        card_id: &str,
        actor_id: &str,
    ) -> Result<CardView, BoardOperationError> {
        let current: Option<(String, String)> =
            sqlx::query_as("SELECT board_id, column_id FROM collab_cards WHERE id = $1")
                .bind(card_id)
                .fetch_optional(&mut **transaction)
                .await?;
        let Some((board_id, column_id)) = current else {
            return Err(domain("NOT_FOUND", "open Card does not exist"));
        };
        lock_board(transaction, &board_id).await?;
        lock_columns(transaction, std::slice::from_ref(&column_id)).await?;
        let assignee: Option<Option<String>> = sqlx::query_scalar(
            "SELECT card.assignee_id
             FROM collab_cards card
             JOIN collab_board_columns board_column
               ON board_column.id = card.column_id AND NOT board_column.is_terminal
             WHERE card.id = $1 FOR UPDATE OF card",
        )
        .bind(card_id)
        .fetch_optional(&mut **transaction)
        .await?;
        let Some(assignee) = assignee else {
            return Err(domain("NOT_FOUND", "open Card does not exist"));
        };
        match assignee.as_deref() {
            Some(current) if current == actor_id => {}
            Some(_) => return Err(domain("CONFLICT", "Card is already assigned")),
            None => {
                sqlx::query(
                    "UPDATE collab_cards
                     SET assignee_id = $2,
                         updated_at = CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai'
                     WHERE id = $1",
                )
                .bind(card_id)
                .bind(actor_id)
                .execute(&mut **transaction)
                .await?;
            }
        }
        card(transaction, card_id).await.map_err(Into::into)
    }

    pub(crate) async fn move_card_in(
        transaction: &mut Transaction<'_, Postgres>,
        card_id: &str,
        target_column_id: &str,
        before_card_id: Option<&str>,
    ) -> Result<CardView, BoardOperationError> {
        sqlx::query("SET CONSTRAINTS collab_cards_position_unique DEFERRED")
            .execute(&mut **transaction)
            .await?;
        let current: Option<(String, String)> =
            sqlx::query_as("SELECT board_id, column_id FROM collab_cards WHERE id = $1")
                .bind(card_id)
                .fetch_optional(&mut **transaction)
                .await?;
        let Some((board_id, source_column_id)) = current else {
            return Err(domain("NOT_FOUND", "Card does not exist"));
        };
        lock_board(transaction, &board_id).await?;
        let mut column_ids = vec![source_column_id.clone(), target_column_id.to_string()];
        column_ids.sort();
        column_ids.dedup();
        let locked = lock_columns(transaction, &column_ids).await?;
        if locked.len() != column_ids.len()
            || !locked
                .iter()
                .any(|(id, locked_board_id)| id == target_column_id && locked_board_id == &board_id)
        {
            return Err(domain(
                "NOT_FOUND",
                "target Column is not in the Card Board",
            ));
        }
        let current: Option<String> =
            sqlx::query_scalar("SELECT column_id FROM collab_cards WHERE id = $1 FOR UPDATE")
                .bind(card_id)
                .fetch_optional(&mut **transaction)
                .await?;
        if current.as_deref() != Some(source_column_id.as_str()) {
            return Err(domain(
                "CONFLICT",
                "Card moved concurrently; retry the command",
            ));
        }
        let mut source = locked_card_ids(transaction, &source_column_id).await?;
        let mut target = if source_column_id == target_column_id {
            Vec::new()
        } else {
            locked_card_ids(transaction, target_column_id).await?
        };
        source.retain(|id| id != card_id);
        let destination = if source_column_id == target_column_id {
            &mut source
        } else {
            &mut target
        };
        let index = match before_card_id {
            Some(before) => destination
                .iter()
                .position(|id| id == before)
                .ok_or_else(|| domain("NOT_FOUND", "before Card is not in target Column"))?,
            None => destination.len(),
        };
        destination.insert(index, card_id.to_string());
        renumber(transaction, &source_column_id, &source).await?;
        if source_column_id != target_column_id {
            renumber(transaction, target_column_id, &target).await?;
        }
        card(transaction, card_id).await.map_err(Into::into)
    }
}

async fn card(
    transaction: &mut Transaction<'_, Postgres>,
    card_id: &str,
) -> Result<CardView, sqlx::Error> {
    sqlx::query_as::<_, CardRow>(
        "SELECT id, board_id, column_id, title, description, position,
                assignee_id, created_by
         FROM collab_cards WHERE id = $1",
    )
    .bind(card_id)
    .fetch_one(&mut **transaction)
    .await
    .map(CardView::from)
}

async fn lock_board(
    transaction: &mut Transaction<'_, Postgres>,
    board_id: &str,
) -> Result<(), BoardOperationError> {
    let locked: Option<String> =
        sqlx::query_scalar("SELECT id FROM collab_boards WHERE id = $1 FOR UPDATE")
            .bind(board_id)
            .fetch_optional(&mut **transaction)
            .await?;
    if locked.is_none() {
        return Err(domain("NOT_FOUND", "Board does not exist"));
    }
    Ok(())
}

async fn board_id_for_column(
    transaction: &mut Transaction<'_, Postgres>,
    column_id: &str,
) -> Result<String, BoardOperationError> {
    sqlx::query_scalar("SELECT board_id FROM collab_board_columns WHERE id = $1")
        .bind(column_id)
        .fetch_optional(&mut **transaction)
        .await?
        .ok_or_else(|| domain("NOT_FOUND", "Column does not exist"))
}

async fn lock_columns(
    transaction: &mut Transaction<'_, Postgres>,
    column_ids: &[String],
) -> Result<Vec<(String, String)>, sqlx::Error> {
    sqlx::query_as(
        "SELECT id, board_id FROM collab_board_columns
         WHERE id = ANY($1) ORDER BY id FOR UPDATE",
    )
    .bind(column_ids)
    .fetch_all(&mut **transaction)
    .await
}

async fn locked_column_ids(
    transaction: &mut Transaction<'_, Postgres>,
    board_id: &str,
) -> Result<Vec<String>, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT id FROM collab_board_columns WHERE board_id = $1
         ORDER BY position, id FOR UPDATE",
    )
    .bind(board_id)
    .fetch_all(&mut **transaction)
    .await
}

async fn renumber_columns(
    transaction: &mut Transaction<'_, Postgres>,
    board_id: &str,
    column_ids: &[String],
) -> Result<(), BoardOperationError> {
    for (position, column_id) in column_ids.iter().enumerate() {
        sqlx::query(
            "UPDATE collab_board_columns SET position = $1
             WHERE id = $2 AND board_id = $3",
        )
        .bind(position as i32)
        .bind(column_id)
        .bind(board_id)
        .execute(&mut **transaction)
        .await?;
    }
    Ok(())
}

async fn touch_board(
    transaction: &mut Transaction<'_, Postgres>,
    board_id: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE collab_boards
         SET updated_at = CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai'
         WHERE id = $1",
    )
    .bind(board_id)
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

async fn locked_card_ids(
    transaction: &mut Transaction<'_, Postgres>,
    column_id: &str,
) -> Result<Vec<String>, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT id FROM collab_cards WHERE column_id = $1
         ORDER BY position, id FOR UPDATE",
    )
    .bind(column_id)
    .fetch_all(&mut **transaction)
    .await
}

async fn renumber(
    transaction: &mut Transaction<'_, Postgres>,
    column_id: &str,
    card_ids: &[String],
) -> Result<(), BoardOperationError> {
    for (position, card_id) in card_ids.iter().enumerate() {
        sqlx::query(
            "UPDATE collab_cards
             SET column_id = $1, position = $2,
                 updated_at = CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai'
             WHERE id = $3",
        )
        .bind(column_id)
        .bind(position as i32)
        .bind(card_id)
        .execute(&mut **transaction)
        .await?;
    }
    Ok(())
}

async fn ensure_active_participant(
    transaction: &mut Transaction<'_, Postgres>,
    participant_id: &str,
) -> Result<(), BoardOperationError> {
    let active: bool = sqlx::query_scalar(
        "SELECT EXISTS(
            SELECT 1 FROM collab_participants participant
            LEFT JOIN collab_agent_profiles profile ON profile.agent_id = participant.id
            WHERE participant.id = $1
              AND (participant.kind = 'user' OR profile.archived_at IS NULL)
         )",
    )
    .bind(participant_id)
    .fetch_one(&mut **transaction)
    .await?;
    if active {
        Ok(())
    } else {
        Err(domain("NOT_FOUND", "assignee is not active"))
    }
}

fn normalize_optional(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| !value.is_empty())
}

#[derive(FromRow)]
struct BoardRow {
    board_id: String,
    board_title: String,
    board_description: Option<String>,
    board_created_by: String,
    column_id: Option<String>,
    column_title: Option<String>,
    column_position: Option<i32>,
    is_terminal: Option<bool>,
    card_id: Option<String>,
    card_column_id: Option<String>,
    card_title: Option<String>,
    card_description: Option<String>,
    card_position: Option<i32>,
    assignee_id: Option<String>,
    card_created_by: Option<String>,
}

#[derive(FromRow)]
struct CardRow {
    id: String,
    board_id: String,
    column_id: String,
    title: String,
    description: Option<String>,
    position: i32,
    assignee_id: Option<String>,
    created_by: String,
}

impl From<CardRow> for CardView {
    fn from(row: CardRow) -> Self {
        Self {
            id: row.id,
            board_id: row.board_id,
            column_id: row.column_id,
            title: row.title,
            description: row.description,
            position: row.position,
            assignee_id: row.assignee_id,
            created_by: row.created_by,
        }
    }
}

fn assemble(rows: Vec<BoardRow>) -> Vec<BoardView> {
    let mut boards = BTreeMap::<String, BoardView>::new();
    for row in rows {
        let board = boards
            .entry(row.board_id.clone())
            .or_insert_with(|| BoardView {
                id: row.board_id,
                title: row.board_title,
                description: row.board_description,
                created_by: row.board_created_by,
                columns: Vec::new(),
            });
        let Some(column_id) = row.column_id else {
            continue;
        };
        if board
            .columns
            .last()
            .is_none_or(|column| column.id != column_id)
        {
            board.columns.push(BoardColumnView {
                id: column_id.clone(),
                title: row.column_title.expect("Column title is non-null"),
                position: row.column_position.expect("Column position is non-null"),
                is_terminal: row.is_terminal.expect("Column terminal flag is non-null"),
                cards: Vec::new(),
            });
        }
        if let (Some(id), Some(column_id), Some(title), Some(position), Some(created_by)) = (
            row.card_id,
            row.card_column_id,
            row.card_title,
            row.card_position,
            row.card_created_by,
        ) {
            board
                .columns
                .last_mut()
                .expect("column was inserted")
                .cards
                .push(CardView {
                    id,
                    board_id: board.id.clone(),
                    column_id,
                    title,
                    description: row.card_description,
                    position,
                    assignee_id: row.assignee_id,
                    created_by,
                });
        }
    }
    boards.into_values().collect()
}

fn protocol_error(message: &str) -> sqlx::Error {
    sqlx::Error::Protocol(message.to_string())
}

fn valid_title<'a>(
    value: &'a str,
    max_bytes: usize,
    message: &'static str,
) -> Result<&'a str, BoardOperationError> {
    let value = value.trim();
    if value.is_empty() || value.len() > max_bytes {
        return Err(domain("INVALID_ARGUMENT", message));
    }
    Ok(value)
}

fn domain(code: &'static str, message: &'static str) -> BoardOperationError {
    BoardOperationError::Domain { code, message }
}
