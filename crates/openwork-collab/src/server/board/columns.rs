//! Column 结构：Desktop 用户创建、重命名、设置 `kind`、重排和删除空 Column（collaboration.md §11.1、§11.2）。
//! Agent 不经过这里。

use sqlx::{Postgres, Transaction};

use super::{Board, BoardOperationError, domain, lock_board, lock_columns, valid_title};
use crate::protocol::{BoardView, ColumnKind, entity_id};

impl Board {
    pub(crate) async fn create_column_in(
        transaction: &mut Transaction<'_, Postgres>,
        board_id: &str,
        title: &str,
        kind: Option<ColumnKind>,
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
            "INSERT INTO collab_board_columns (id, board_id, title, position, kind)
             VALUES ($1, $2, $3, $4, $5)",
        )
        .bind(entity_id("col"))
        .bind(board_id)
        .bind(title)
        .bind(position)
        .bind(kind.map(ColumnKind::as_str))
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
        kind: Option<ColumnKind>,
    ) -> Result<BoardView, BoardOperationError> {
        let title = valid_title(title, 200, "Column title must be 1..200 bytes")?;
        let board_id = board_id_for_column(transaction, column_id).await?;
        lock_board(transaction, &board_id).await?;
        let updated = sqlx::query(
            "UPDATE collab_board_columns
             SET title = $2, kind = $3
             WHERE id = $1 AND board_id = $4",
        )
        .bind(column_id)
        .bind(title)
        .bind(kind.map(ColumnKind::as_str))
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
