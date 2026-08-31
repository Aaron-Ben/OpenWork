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
             JOIN collab_board_columns board_column ON board_column.board_id = board.id
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
             JOIN collab_board_columns board_column ON board_column.board_id = board.id
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
             JOIN collab_board_columns board_column ON board_column.board_id = board.id
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
        let title = title.trim();
        if title.is_empty() || title.len() > 500 {
            return Err(domain(
                "INVALID_ARGUMENT",
                "card title must be 1..500 bytes",
            ));
        }
        let column_exists: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM collab_board_columns
             WHERE id = $1 AND board_id = $2)",
        )
        .bind(column_id)
        .bind(board_id)
        .fetch_one(&mut **transaction)
        .await?;
        if !column_exists {
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

    pub(crate) async fn claim_card_in(
        transaction: &mut Transaction<'_, Postgres>,
        card_id: &str,
        actor_id: &str,
    ) -> Result<CardView, BoardOperationError> {
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
        let current: Option<(String, String)> = sqlx::query_as(
            "SELECT board_id, column_id FROM collab_cards
             WHERE id = $1 FOR UPDATE",
        )
        .bind(card_id)
        .fetch_optional(&mut **transaction)
        .await?;
        let Some((board_id, source_column_id)) = current else {
            return Err(domain("NOT_FOUND", "Card does not exist"));
        };
        let target_exists: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM collab_board_columns
             WHERE id = $1 AND board_id = $2)",
        )
        .bind(target_column_id)
        .bind(&board_id)
        .fetch_one(&mut **transaction)
        .await?;
        if !target_exists {
            return Err(domain(
                "NOT_FOUND",
                "target Column is not in the Card Board",
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
    column_id: String,
    column_title: String,
    column_position: i32,
    is_terminal: bool,
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
        if board
            .columns
            .last()
            .is_none_or(|column| column.id != row.column_id)
        {
            board.columns.push(BoardColumnView {
                id: row.column_id.clone(),
                title: row.column_title,
                position: row.column_position,
                is_terminal: row.is_terminal,
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

fn domain(code: &'static str, message: &'static str) -> BoardOperationError {
    BoardOperationError::Domain { code, message }
}
