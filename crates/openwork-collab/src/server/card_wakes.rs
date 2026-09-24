//! 卡片唤醒（collaboration.md §11.4、§13.3.6）：改派与新增 `@` 写入 `collab_card_wakes`，同一 Agent
//! 同一张卡片合并为一条待处理记录；收件箱带出待处理记录并以 `card` 为 trigger，Run 打开时认领版本号
//! 未变的记录，Run 成功后结算。限额与 SSE 发布由调用方负责。

use sqlx::{FromRow, PgPool, Postgres, Transaction};

use super::{auth::AgentClaims, routing::mentions, runs::Runs};
use crate::protocol::{
    CardView, CardWakeReason, CardWakeRef, CardWakeView, ColumnKind, InboxResponse, entity_id,
};

/// Card wakes carried by one card Turn; the rest wait for the next Turn (collaboration.md §11.4).
const CARD_TURN_MAX_CARDS: i64 = 10;

#[derive(Clone)]
pub(crate) struct CardWakes {
    pool: PgPool,
}

impl CardWakes {
    pub(crate) fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// 这次卡片变化应该唤醒的 active Agent 与原因；`before` 为 `None` 表示新建。
    pub(crate) async fn targets_in(
        transaction: &mut Transaction<'_, Postgres>,
        before: Option<&CardView>,
        after: &CardView,
        actor_id: &str,
    ) -> Result<Vec<(String, CardWakeReason)>, sqlx::Error> {
        let active: Vec<String> = sqlx::query_scalar(
            "SELECT agent_id FROM collab_agent_profiles WHERE archived_at IS NULL ORDER BY agent_id",
        )
        .fetch_all(&mut **transaction)
        .await?;
        Ok(wake_targets(before, after, actor_id, &active))
    }

    /// 写入一条待处理唤醒；已有待处理记录时合并：更新原因、版本号加 1，并让它脱离正在处理它的 Run。
    pub(crate) async fn record_in(
        transaction: &mut Transaction<'_, Postgres>,
        card_id: &str,
        agent_id: &str,
        reason: CardWakeReason,
    ) -> Result<(), sqlx::Error> {
        sqlx::query(
            "INSERT INTO collab_card_wakes (id, agent_id, card_id, reason)
             VALUES ($1, $2, $3, $4)
             ON CONFLICT (agent_id, card_id) WHERE settled_at IS NULL
             DO UPDATE SET reason = EXCLUDED.reason,
                           revision = collab_card_wakes.revision + 1,
                           run_id = NULL,
                           updated_at = CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai'",
        )
        .bind(entity_id("cardwake"))
        .bind(agent_id)
        .bind(card_id)
        .bind(reason.as_str())
        .execute(&mut **transaction)
        .await?;
        Ok(())
    }

    /// 给收件箱带上该 Agent 的待处理卡片唤醒。有待处理记录时 trigger 改为 `card`（没有未读消息时新建
    /// 一个不带 delivery 的 trigger），并列出这些记录的 id 与版本号；返回的 trigger 尚未签名。
    pub(crate) async fn with_pending(
        &self,
        claims: &AgentClaims,
        mut inbox: InboxResponse,
    ) -> Result<InboxResponse, sqlx::Error> {
        let (cards, total) = self.pending(&claims.sub).await?;
        if cards.is_empty() {
            return Ok(inbox);
        }
        let mut trigger = inbox
            .trigger
            .take()
            .unwrap_or_else(|| Runs::unsigned_trigger(claims, Vec::new(), false));
        trigger.trigger = "card".to_string();
        trigger.card_wakes = cards
            .iter()
            .map(|card| CardWakeRef {
                id: card.id.clone(),
                revision: card.revision,
            })
            .collect();
        inbox.trigger = Some(trigger);
        inbox.more_cards = total - cards.len() as i64;
        inbox.cards = cards;
        Ok(inbox)
    }

    /// 最早写入的至多 10 条待处理唤醒，以及待处理的总数。
    async fn pending(&self, agent_id: &str) -> Result<(Vec<CardWakeView>, i64), sqlx::Error> {
        let rows = sqlx::query_as::<_, PendingRow>(
            "SELECT COUNT(*) OVER () AS total, wake.id, wake.revision, wake.reason, card.id AS card_id,
                    card.title AS card_title, board.id AS board_id, board.title AS board_title,
                    board_column.id AS column_id, board_column.title AS column_title,
                    board_column.kind AS column_kind, card.assignee_id
             FROM collab_card_wakes wake
             JOIN collab_cards card ON card.id = wake.card_id
             JOIN collab_boards board ON board.id = card.board_id
             JOIN collab_board_columns board_column ON board_column.id = card.column_id
             WHERE wake.agent_id = $1 AND wake.settled_at IS NULL
             ORDER BY wake.created_at, wake.id
             LIMIT $2",
        )
        .bind(agent_id)
        .bind(CARD_TURN_MAX_CARDS)
        .fetch_all(&self.pool)
        .await?;
        let total = rows.first().map_or(0, |row| row.total);
        let cards = rows
            .into_iter()
            .map(CardWakeView::try_from)
            .collect::<Result<Vec<_>, _>>()?;
        Ok((cards, total))
    }

    /// Run 打开时认领 trigger 列出、且版本号未变的待处理记录。读收件箱之后才合并的记录版本号已变，
    /// 留给下一个 Run（§13.3.6）。
    pub(crate) async fn attach_in(
        transaction: &mut Transaction<'_, Postgres>,
        run_id: &str,
        agent_id: &str,
        wakes: &[CardWakeRef],
    ) -> Result<(), sqlx::Error> {
        if wakes.is_empty() {
            return Ok(());
        }
        let ids = wakes.iter().map(|wake| wake.id.clone()).collect::<Vec<_>>();
        let revisions = wakes.iter().map(|wake| wake.revision).collect::<Vec<_>>();
        sqlx::query(
            "UPDATE collab_card_wakes wake
             SET run_id = $1
             FROM UNNEST($3::TEXT[], $4::INTEGER[]) AS listed(id, revision)
             WHERE wake.id = listed.id AND wake.revision = listed.revision
               AND wake.agent_id = $2 AND wake.settled_at IS NULL",
        )
        .bind(run_id)
        .bind(agent_id)
        .bind(&ids)
        .bind(&revisions)
        .execute(&mut **transaction)
        .await?;
        Ok(())
    }

    /// Run 成功后结算仍指向它的记录；Run 进行中又合并过的记录已脱离，不在其中。
    pub(crate) async fn settle_in(
        transaction: &mut Transaction<'_, Postgres>,
        run_id: &str,
    ) -> Result<(), sqlx::Error> {
        sqlx::query(
            "UPDATE collab_card_wakes
             SET settled_at = CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai'
             WHERE run_id = $1 AND settled_at IS NULL",
        )
        .bind(run_id)
        .execute(&mut **transaction)
        .await?;
        Ok(())
    }
}

/// 改派给别人以外的 active Agent（负责人真的变了）记为 `assigned`；标题或描述中新增的 `@<id>` 记为
/// `mentioned`；同一 Agent 两者都有时记为 `assigned`。发起者与不在 `active` 中的 Agent 不唤醒。
fn wake_targets(
    before: Option<&CardView>,
    after: &CardView,
    actor_id: &str,
    active: &[String],
) -> Vec<(String, CardWakeReason)> {
    let before_text = before.map(card_text).unwrap_or_default();
    let after_text = card_text(after);
    let previous_assignee = before.and_then(|card| card.assignee_id.as_deref());
    active
        .iter()
        .filter(|agent_id| agent_id.as_str() != actor_id)
        .filter_map(|agent_id| {
            let assigned = after.assignee_id.as_deref() == Some(agent_id.as_str())
                && previous_assignee != Some(agent_id.as_str());
            let mentioned = mentions(&after_text, agent_id) && !mentions(&before_text, agent_id);
            match (assigned, mentioned) {
                (true, _) => Some((agent_id.clone(), CardWakeReason::Assigned)),
                (false, true) => Some((agent_id.clone(), CardWakeReason::Mentioned)),
                (false, false) => None,
            }
        })
        .collect()
}

fn card_text(card: &CardView) -> String {
    format!(
        "{}\n{}",
        card.title,
        card.description.as_deref().unwrap_or("")
    )
}

#[derive(FromRow)]
struct PendingRow {
    total: i64,
    id: String,
    revision: i32,
    reason: String,
    card_id: String,
    card_title: String,
    board_id: String,
    board_title: String,
    column_id: String,
    column_title: String,
    column_kind: Option<String>,
    assignee_id: Option<String>,
}

impl TryFrom<PendingRow> for CardWakeView {
    type Error = sqlx::Error;

    fn try_from(row: PendingRow) -> Result<Self, Self::Error> {
        let reason = CardWakeReason::parse(&row.reason).ok_or_else(|| {
            sqlx::Error::Decode(format!("unknown card wake reason {:?}", row.reason).into())
        })?;
        Ok(Self {
            id: row.id,
            revision: row.revision,
            reason,
            card_id: row.card_id,
            card_title: row.card_title,
            board_id: row.board_id,
            board_title: row.board_title,
            column_id: row.column_id,
            column_title: row.column_title,
            column_kind: row.column_kind.as_deref().and_then(ColumnKind::parse),
            assignee_id: row.assignee_id,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn card(assignee: Option<&str>, title: &str, description: Option<&str>) -> CardView {
        CardView {
            id: "card-1".to_string(),
            board_id: "board-1".to_string(),
            column_id: "col-1".to_string(),
            title: title.to_string(),
            description: description.map(str::to_string),
            position: 0,
            assignee_id: assignee.map(str::to_string),
            created_by: "ada".to_string(),
            agent_state: None,
        }
    }

    fn active() -> Vec<String> {
        ["ada", "bo", "cy"].map(str::to_string).to_vec()
    }

    #[test]
    fn acc_14_only_real_reassignments_and_new_mentions_wake() {
        let plain = card(Some("bo"), "Fix login", None);
        // 新建时指定负责人算改派；重复提交同一负责人不算。
        assert_eq!(
            wake_targets(None, &plain, "ada", &active()),
            vec![("bo".to_string(), CardWakeReason::Assigned)]
        );
        assert!(wake_targets(Some(&plain), &plain, "ada", &active()).is_empty());
        // 新增的 @ 唤醒，已有的 @ 不再唤醒；`@cyx` 不是 `@cy`。
        let mentioned = card(Some("bo"), "Fix login", Some("ask @cy and @cyx"));
        assert_eq!(
            wake_targets(Some(&plain), &mentioned, "ada", &active()),
            vec![("cy".to_string(), CardWakeReason::Mentioned)]
        );
        let reworded = card(Some("bo"), "Fix login @cy", Some("ask @cy again"));
        assert!(wake_targets(Some(&mentioned), &reworded, "ada", &active()).is_empty());
    }

    #[test]
    fn acc_14_the_initiator_and_inactive_agents_are_not_woken() {
        let before = card(None, "Fix login", None);
        let claimed = card(Some("ada"), "Fix login @ada @gone", None);
        assert!(wake_targets(Some(&before), &claimed, "ada", &active()).is_empty());
        let both = card(Some("cy"), "Fix login @cy", None);
        assert_eq!(
            wake_targets(Some(&before), &both, "ada", &active()),
            vec![("cy".to_string(), CardWakeReason::Assigned)]
        );
        let to_user = card(Some("local-user"), "Fix login", None);
        assert!(wake_targets(Some(&before), &to_user, "ada", &active()).is_empty());
    }
}
