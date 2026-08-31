use sqlx::{FromRow, PgPool};
use uuid::Uuid;

use crate::protocol::{BoardColumnView, BoardView, CardView};

#[derive(Clone)]
pub(crate) struct Board {
    pool: PgPool,
}

impl Board {
    pub(crate) fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub(crate) async fn create(
        &self,
        room_id: &str,
        title: &str,
    ) -> Result<BoardView, sqlx::Error> {
        if title.trim().is_empty() || title.len() > 200 {
            return Err(protocol_error(
                "INVALID_ARGUMENT: board title must be 1..200 bytes",
            ));
        }
        let board_id = format!("board_{}", Uuid::new_v4().simple());
        let mut transaction = self.pool.begin().await?;
        sqlx::query("INSERT INTO collab_boards (id, room_id, title) VALUES ($1, $2, $3)")
            .bind(&board_id)
            .bind(room_id)
            .bind(title.trim())
            .execute(&mut *transaction)
            .await?;
        for (position, title, is_done) in [
            (0_i32, "To do", false),
            (1, "Doing", false),
            (2, "Done", true),
        ] {
            sqlx::query(
                "INSERT INTO collab_board_columns (id, board_id, title, position, is_done)
                 VALUES ($1, $2, $3, $4, $5)",
            )
            .bind(format!("column_{}", Uuid::new_v4().simple()))
            .bind(&board_id)
            .bind(title)
            .bind(position)
            .bind(is_done)
            .execute(&mut *transaction)
            .await?;
        }
        transaction.commit().await?;
        self.get(&board_id).await
    }

    pub(crate) async fn list(&self) -> Result<Vec<BoardView>, sqlx::Error> {
        let ids: Vec<String> =
            sqlx::query_scalar("SELECT id FROM collab_boards ORDER BY created_at, id")
                .fetch_all(&self.pool)
                .await?;
        let mut boards = Vec::with_capacity(ids.len());
        for id in ids {
            boards.push(self.get(&id).await?);
        }
        Ok(boards)
    }

    async fn get(&self, board_id: &str) -> Result<BoardView, sqlx::Error> {
        let (id, room_id, title): (String, String, String) =
            sqlx::query_as("SELECT id, room_id, title FROM collab_boards WHERE id = $1")
                .bind(board_id)
                .fetch_one(&self.pool)
                .await?;
        let columns = sqlx::query_as::<_, BoardColumnRow>(
            "SELECT id, title, position, is_done
             FROM collab_board_columns WHERE board_id = $1 ORDER BY position, id",
        )
        .bind(board_id)
        .fetch_all(&self.pool)
        .await?;
        let mut views = Vec::with_capacity(columns.len());
        for column in columns {
            let cards = sqlx::query_as::<_, CardRow>(
                "SELECT id, title, description, position, assignee_id, claimed_by
                 FROM collab_cards WHERE board_id = $1 AND column_id = $2
                 ORDER BY position, id",
            )
            .bind(board_id)
            .bind(&column.id)
            .fetch_all(&self.pool)
            .await?
            .into_iter()
            .map(CardView::from)
            .collect();
            views.push(BoardColumnView {
                id: column.id,
                title: column.title,
                position: column.position,
                is_done: column.is_done,
                cards,
            });
        }
        Ok(BoardView {
            id,
            room_id,
            title,
            columns: views,
        })
    }
}

#[derive(FromRow)]
struct BoardColumnRow {
    id: String,
    title: String,
    position: i32,
    is_done: bool,
}

#[derive(FromRow)]
struct CardRow {
    id: String,
    title: String,
    description: Option<String>,
    position: i32,
    assignee_id: Option<String>,
    claimed_by: Option<String>,
}

impl From<CardRow> for CardView {
    fn from(row: CardRow) -> Self {
        Self {
            id: row.id,
            title: row.title,
            description: row.description,
            position: row.position,
            assignee_id: row.assignee_id,
            claimed_by: row.claimed_by,
        }
    }
}

fn protocol_error(message: &str) -> sqlx::Error {
    sqlx::Error::Protocol(message.to_string())
}
