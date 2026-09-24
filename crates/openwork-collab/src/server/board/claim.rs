//! Card 领取（collaboration.md §11.3）：在一个事务里复核负责人与接手条件，成功后把 `todo`
//! 列的卡片推进到最左的 `doing` 列。改派与卡片唤醒不在这里。

use sqlx::{FromRow, Postgres, Transaction};

use super::{Board, BoardOperationError, domain, lock_board, lock_columns};
use crate::protocol::{CardView, ColumnKind};

/// 负责人多久没更新卡片后，别的 Agent 可以接手（collaboration.md §11.3，取自 Cumora `cli.ts` 的 `card claim`）。
const TAKEOVER_IDLE_MINUTES: i32 = 20;

struct ColumnShape {
    id: String,
    position: i32,
    kind: Option<ColumnKind>,
}

/// 在卡片行锁内读到的负责人状态。
#[derive(FromRow)]
struct Holder {
    assignee_id: Option<String>,
    assignee_archived: bool,
    idle: bool,
    assignee_running: bool,
}

#[derive(Debug, PartialEq, Eq)]
enum Refusal {
    Done,
    Held(String),
}

impl Board {
    /// 让 `actor_id` 领取卡片。可领取时写入负责人（已是自己时不改），卡片在 `todo` 列时追加到
    /// 最左 `doing` 列的末尾；返回领取后的卡片。卡片在 `done` 列或仍由别人负责时返回 `CONFLICT`，
    /// 卡片不存在时返回 `NOT_FOUND`，两种情况都不写入。
    ///
    /// 加锁顺序为 Board → Column ID → Card（§13.6 #4）；持有 Board 锁后卡片所在列与 Board 的列结构
    /// 都不会再变，所以可以先算出目标列再加锁。
    pub(crate) async fn claim_card_in(
        transaction: &mut Transaction<'_, Postgres>,
        card_id: &str,
        actor_id: &str,
    ) -> Result<CardView, BoardOperationError> {
        let board_id: Option<String> =
            sqlx::query_scalar("SELECT board_id FROM collab_cards WHERE id = $1")
                .bind(card_id)
                .fetch_optional(&mut **transaction)
                .await?;
        let Some(board_id) = board_id else {
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
        let columns = board_columns(transaction, &board_id).await?;
        let target = claim_target(&columns, &column_id).map(str::to_string);
        let mut column_ids: Vec<String> = [Some(column_id.clone()), target.clone()]
            .into_iter()
            .flatten()
            .collect();
        column_ids.sort();
        lock_columns(transaction, &column_ids).await?;
        let holder = locked_holder(transaction, card_id).await?;
        let current_kind = columns
            .iter()
            .find(|column| column.id == column_id)
            .and_then(|column| column.kind);
        if let Some(refusal) = refusal(current_kind, &holder, actor_id) {
            return Err(refusal_error(card_id, refusal));
        }
        if holder.assignee_id.as_deref() != Some(actor_id) {
            assign(transaction, card_id, actor_id).await?;
        }
        match target {
            Some(target) => Self::move_card_in(transaction, card_id, &target, None).await,
            None => super::card(transaction, card_id).await.map_err(Into::into),
        }
    }
}

/// 领取时卡片应推进到的列；`None` 表示不动。只从 `todo` 推进，有多个 `doing` 时取最左的；
/// 在 `done`、未分类列或没有 `doing` 列时不动，所以只前进、不后退（Cumora `board-columns.ts` 的
/// `claimTargetColumn`）。
fn claim_target<'a>(columns: &'a [ColumnShape], current_column_id: &str) -> Option<&'a str> {
    let current = columns
        .iter()
        .find(|column| column.id == current_column_id)?;
    if current.kind != Some(ColumnKind::Todo) {
        return None;
    }
    columns
        .iter()
        .filter(|column| column.kind == Some(ColumnKind::Doing))
        .min_by_key(|column| column.position)
        .map(|column| column.id.as_str())
}

/// 不能领取的原因；可以领取时返回 `None`。负责人是自己时幂等成功；别人负责时，只有负责人已归档，
/// 或卡片超过 20 分钟没更新且负责人没有 running Run，才可以接手。
fn refusal(current_kind: Option<ColumnKind>, holder: &Holder, actor_id: &str) -> Option<Refusal> {
    if current_kind == Some(ColumnKind::Done) {
        return Some(Refusal::Done);
    }
    let assignee = holder.assignee_id.as_deref()?;
    let takeover = holder.assignee_archived || (holder.idle && !holder.assignee_running);
    (assignee != actor_id && !takeover).then(|| Refusal::Held(assignee.to_string()))
}

fn refusal_error(card_id: &str, refusal: Refusal) -> BoardOperationError {
    let message = match refusal {
        Refusal::Done => {
            format!("card {card_id} is in a done column; it is finished, so pick another card.")
        }
        Refusal::Held(holder) => format!(
            "card {card_id} is already being worked by @{holder} — move on to another card."
        ),
    };
    domain("CONFLICT", message)
}

async fn board_columns(
    transaction: &mut Transaction<'_, Postgres>,
    board_id: &str,
) -> Result<Vec<ColumnShape>, sqlx::Error> {
    let rows: Vec<(String, i32, Option<String>)> =
        sqlx::query_as("SELECT id, position, kind FROM collab_board_columns WHERE board_id = $1")
            .bind(board_id)
            .fetch_all(&mut **transaction)
            .await?;
    Ok(rows
        .into_iter()
        .map(|(id, position, kind)| ColumnShape {
            id,
            position,
            kind: kind.as_deref().and_then(ColumnKind::parse),
        })
        .collect())
}

async fn locked_holder(
    transaction: &mut Transaction<'_, Postgres>,
    card_id: &str,
) -> Result<Holder, sqlx::Error> {
    sqlx::query_as(
        "SELECT card.assignee_id,
                COALESCE(profile.archived_at IS NOT NULL, FALSE) AS assignee_archived,
                card.updated_at < (CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai')
                    - make_interval(mins => $2) AS idle,
                EXISTS(
                    SELECT 1 FROM collab_runs run
                    WHERE run.agent_id = card.assignee_id AND run.status = 'running'
                ) AS assignee_running
         FROM collab_cards card
         LEFT JOIN collab_agent_profiles profile ON profile.agent_id = card.assignee_id
         WHERE card.id = $1
         FOR UPDATE OF card",
    )
    .bind(card_id)
    .bind(TAKEOVER_IDLE_MINUTES)
    .fetch_one(&mut **transaction)
    .await
}

async fn assign(
    transaction: &mut Transaction<'_, Postgres>,
    card_id: &str,
    actor_id: &str,
) -> Result<(), sqlx::Error> {
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
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn column(id: &str, position: i32, kind: Option<ColumnKind>) -> ColumnShape {
        ColumnShape {
            id: id.to_string(),
            position,
            kind,
        }
    }

    fn holder(assignee: Option<&str>, archived: bool, idle: bool, running: bool) -> Holder {
        Holder {
            assignee_id: assignee.map(str::to_string),
            assignee_archived: archived,
            idle,
            assignee_running: running,
        }
    }

    #[test]
    fn acc_13_claim_moves_only_from_todo_to_the_leftmost_doing() {
        let columns = [
            column("todo", 0, Some(ColumnKind::Todo)),
            column("later", 3, Some(ColumnKind::Doing)),
            column("doing", 1, Some(ColumnKind::Doing)),
            column("done", 2, Some(ColumnKind::Done)),
            column("backlog", 4, None),
        ];
        assert_eq!(claim_target(&columns, "todo"), Some("doing"));
        assert_eq!(claim_target(&columns, "doing"), None);
        assert_eq!(claim_target(&columns, "later"), None);
        assert_eq!(claim_target(&columns, "done"), None);
        assert_eq!(claim_target(&columns, "backlog"), None);
        assert_eq!(claim_target(&columns, "missing"), None);
        let no_doing = [
            column("todo", 0, Some(ColumnKind::Todo)),
            column("review", 1, None),
        ];
        assert_eq!(claim_target(&no_doing, "todo"), None);
    }

    #[test]
    fn acc_13_takeover_needs_an_archived_holder_or_an_idle_card_without_a_running_run() {
        let todo = Some(ColumnKind::Todo);
        assert_eq!(
            refusal(todo, &holder(None, false, false, false), "me"),
            None
        );
        assert_eq!(
            refusal(todo, &holder(Some("me"), false, false, true), "me"),
            None
        );
        assert_eq!(
            refusal(None, &holder(Some("bo"), true, false, true), "me"),
            None
        );
        assert_eq!(
            refusal(todo, &holder(Some("bo"), false, true, false), "me"),
            None
        );
        let held = Some(Refusal::Held("bo".to_string()));
        assert_eq!(
            refusal(todo, &holder(Some("bo"), false, false, false), "me"),
            held
        );
        assert_eq!(
            refusal(todo, &holder(Some("bo"), false, true, true), "me"),
            held
        );
        assert_eq!(
            refusal(
                Some(ColumnKind::Done),
                &holder(None, false, false, false),
                "me"
            ),
            Some(Refusal::Done)
        );
    }
}
