//! Agent 当前在做什么（collaboration-desktop.md §4.1），以及卡片的 `agentState`（§4.3）。
//! 数据库给出正在跑的 Run、待处理的卡片唤醒与最近一次发言；Runner heartbeat 给出出错。
//! 只供 Desktop 读取；Agent 命令与模型看到的输出不经过这里。

use std::collections::{HashMap, HashSet};

use sqlx::{FromRow, PgConnection};

use super::agents::AgentRecord;
use crate::protocol::{
    AgentActivity, AgentView, BoardColumnView, BoardView, CardAgentState, CardView, RunnerState,
    RunnerStatusView,
};

/// 数据库里关于一个 Agent 当前状态的事实。
#[derive(Default)]
struct Facts {
    working: Option<WorkingRow>,
    queued: Option<QueuedRow>,
    last_spoke: Option<LastSpokeRow>,
}

#[derive(FromRow)]
struct WorkingRow {
    agent_id: String,
    room_id: Option<String>,
    room_title: Option<String>,
    card_id: Option<String>,
    card_title: Option<String>,
    started_at: String,
}

#[derive(FromRow)]
struct QueuedRow {
    agent_id: String,
    card_count: i64,
    first_card_title: String,
}

#[derive(FromRow)]
struct LastSpokeRow {
    agent_id: String,
    room_id: String,
    room_title: Option<String>,
    last_spoke_at: String,
}

pub(crate) struct Activities;

impl Activities {
    /// 给每个 Agent 补上当前状态，保持输入顺序。
    pub(crate) async fn views_in(
        connection: &mut PgConnection,
        records: Vec<AgentRecord>,
        runners: &[RunnerStatusView],
    ) -> Result<Vec<AgentView>, sqlx::Error> {
        let ids = records
            .iter()
            .map(|record| record.id.clone())
            .collect::<Vec<_>>();
        let mut facts = facts_in(connection, &ids).await?;
        Ok(records
            .into_iter()
            .map(|record| {
                let facts = facts.remove(&record.id).unwrap_or_default();
                let runner = runners.iter().find(|runner| runner.agent_id == record.id);
                let activity = resolve(record.archived_at.is_some(), facts, runner);
                view(record, activity)
            })
            .collect())
    }

    /// 单个 Agent 的 `views_in`。
    pub(crate) async fn view_in(
        connection: &mut PgConnection,
        record: AgentRecord,
        runners: &[RunnerStatusView],
    ) -> Result<AgentView, sqlx::Error> {
        let mut views = Self::views_in(connection, vec![record], runners).await?;
        Ok(views.remove(0))
    }

    /// 给看板里有负责人的卡片补上 `agentState`：负责人的 running Run 正在处理它为 `working`；
    /// 负责人对它有未结算的卡片唤醒时为 `queued`。
    pub(crate) async fn boards_in(
        connection: &mut PgConnection,
        boards: Vec<BoardView>,
    ) -> Result<Vec<BoardView>, sqlx::Error> {
        let working: HashSet<String> = sqlx::query_scalar(
            "SELECT card.id
             FROM collab_cards card
             JOIN collab_runs run ON run.agent_id = card.assignee_id AND run.status = 'running'
             WHERE run.focus_card_id = card.id
                OR EXISTS (
                    SELECT 1 FROM collab_card_wakes wake
                    WHERE wake.run_id = run.id AND wake.card_id = card.id
                )",
        )
        .fetch_all(&mut *connection)
        .await?
        .into_iter()
        .collect();
        let pending: HashSet<String> = sqlx::query_scalar(
            "SELECT wake.card_id
             FROM collab_card_wakes wake
             JOIN collab_cards card ON card.id = wake.card_id AND card.assignee_id = wake.agent_id
             WHERE wake.settled_at IS NULL",
        )
        .fetch_all(&mut *connection)
        .await?
        .into_iter()
        .collect();
        Ok(with_card_states(boards, &working, &pending))
    }
}

/// 按卡片 id 给每张卡片设置 `agent_state`。
fn with_card_states(
    boards: Vec<BoardView>,
    working: &HashSet<String>,
    pending: &HashSet<String>,
) -> Vec<BoardView> {
    let card = |card: CardView| {
        let agent_state = card_state(working.contains(&card.id), pending.contains(&card.id));
        CardView {
            agent_state,
            ..card
        }
    };
    boards
        .into_iter()
        .map(|board| BoardView {
            columns: board
                .columns
                .into_iter()
                .map(|column| BoardColumnView {
                    cards: column.cards.into_iter().map(card).collect(),
                    ..column
                })
                .collect(),
            ..board
        })
        .collect()
}

async fn facts_in(
    connection: &mut PgConnection,
    ids: &[String],
) -> Result<HashMap<String, Facts>, sqlx::Error> {
    let mut facts: HashMap<String, Facts> = HashMap::new();
    let working = sqlx::query_as::<_, WorkingRow>(
        "SELECT DISTINCT ON (run.agent_id) run.agent_id, run.room_id, room.title AS room_title,
                COALESCE(focus.id, wake_card.id) AS card_id,
                COALESCE(focus.title, wake_card.title) AS card_title,
                to_char(run.started_at, 'YYYY-MM-DD\"T\"HH24:MI:SS') || '+08:00' AS started_at
         FROM collab_runs run
         LEFT JOIN collab_rooms room ON room.id = run.room_id
         LEFT JOIN collab_cards focus ON focus.id = run.focus_card_id
         LEFT JOIN LATERAL (
             SELECT card.id, card.title
             FROM collab_card_wakes wake
             JOIN collab_cards card ON card.id = wake.card_id
             WHERE wake.run_id = run.id
             ORDER BY wake.created_at, wake.id
             LIMIT 1
         ) wake_card ON TRUE
         WHERE run.agent_id = ANY($1) AND run.status = 'running'
         ORDER BY run.agent_id, run.started_at DESC",
    )
    .bind(ids)
    .fetch_all(&mut *connection)
    .await?;
    for row in working {
        let agent_id = row.agent_id.clone();
        facts.entry(agent_id).or_default().working = Some(row);
    }
    let queued = sqlx::query_as::<_, QueuedRow>(
        "SELECT wake.agent_id, COUNT(*) AS card_count,
                (array_agg(card.title ORDER BY wake.created_at, wake.id))[1] AS first_card_title
         FROM collab_card_wakes wake
         JOIN collab_cards card ON card.id = wake.card_id
         WHERE wake.agent_id = ANY($1) AND wake.settled_at IS NULL
         GROUP BY wake.agent_id",
    )
    .bind(ids)
    .fetch_all(&mut *connection)
    .await?;
    for row in queued {
        let agent_id = row.agent_id.clone();
        facts.entry(agent_id).or_default().queued = Some(row);
    }
    let last_spoke = sqlx::query_as::<_, LastSpokeRow>(
        "SELECT DISTINCT ON (message.author_id) message.author_id AS agent_id, message.room_id,
                room.title AS room_title,
                to_char(message.created_at, 'YYYY-MM-DD\"T\"HH24:MI:SS') || '+08:00' AS last_spoke_at
         FROM collab_messages message
         JOIN collab_rooms room ON room.id = message.room_id
         WHERE message.author_id = ANY($1) AND message.kind = 'normal'
         ORDER BY message.author_id, message.created_at DESC, message.sequence DESC",
    )
    .bind(ids)
    .fetch_all(&mut *connection)
    .await?;
    for row in last_spoke {
        let agent_id = row.agent_id.clone();
        facts.entry(agent_id).or_default().last_spoke = Some(row);
    }
    Ok(facts)
}

/// 按 归档 → 工作中 → 出错 → 排队 → 空闲 的先后取第一种（collaboration-desktop.md §4.1）。
fn resolve(archived: bool, facts: Facts, runner: Option<&RunnerStatusView>) -> AgentActivity {
    if archived {
        return AgentActivity::Archived;
    }
    if let Some(working) = facts.working {
        return AgentActivity::Working {
            room_id: working.room_id,
            room_title: working.room_title,
            card_id: working.card_id,
            card_title: working.card_title,
            started_at: working.started_at,
        };
    }
    if let Some(runner) = runner.filter(|runner| runner.state == RunnerState::Error) {
        return AgentActivity::Error {
            message: runner.last_error.clone().unwrap_or_default(),
        };
    }
    if let Some(queued) = facts.queued {
        return AgentActivity::Queued {
            card_count: queued.card_count,
            first_card_title: queued.first_card_title,
        };
    }
    let last = facts.last_spoke;
    AgentActivity::Idle {
        room_id: last.as_ref().map(|row| row.room_id.clone()),
        room_title: last.as_ref().and_then(|row| row.room_title.clone()),
        last_spoke_at: last.map(|row| row.last_spoke_at),
    }
}

fn card_state(working: bool, pending: bool) -> Option<CardAgentState> {
    if working {
        Some(CardAgentState::Working)
    } else if pending {
        Some(CardAgentState::Queued)
    } else {
        None
    }
}

fn view(record: AgentRecord, activity: AgentActivity) -> AgentView {
    AgentView {
        id: record.id,
        display_name: record.display_name,
        role: record.role,
        persona: record.persona,
        engine_id: record.engine_id,
        main_model_id: record.main_model_id,
        triage_model_id: record.triage_model_id,
        config_revision: record.config_revision,
        agenda_enabled: record.agenda_enabled,
        archived_at: record.archived_at,
        activity,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn working() -> WorkingRow {
        WorkingRow {
            agent_id: "ada".to_string(),
            room_id: Some("room-1".to_string()),
            room_title: Some("Release".to_string()),
            card_id: None,
            card_title: None,
            started_at: "2026-09-25T10:07:14+08:00".to_string(),
        }
    }

    fn queued() -> QueuedRow {
        QueuedRow {
            agent_id: "ada".to_string(),
            card_count: 2,
            first_card_title: "Fix login".to_string(),
        }
    }

    fn runner(state: RunnerState) -> RunnerStatusView {
        RunnerStatusView {
            agent_id: "ada".to_string(),
            config_revision: 1,
            state,
            last_error: (state == RunnerState::Error).then(|| "Engine missing".to_string()),
        }
    }

    /// collaboration-desktop.md §4.1：归档 → 工作中 → 出错 → 排队 → 空闲。
    #[test]
    fn activity_takes_the_first_matching_state_in_the_documented_order() {
        let all = || Facts {
            working: Some(working()),
            queued: Some(queued()),
            last_spoke: None,
        };
        assert_eq!(resolve(true, all(), None), AgentActivity::Archived);
        assert!(matches!(
            resolve(false, all(), Some(&runner(RunnerState::Error))),
            AgentActivity::Working { .. }
        ));
        let not_working = || Facts {
            working: None,
            ..all()
        };
        assert_eq!(
            resolve(false, not_working(), Some(&runner(RunnerState::Error))),
            AgentActivity::Error {
                message: "Engine missing".to_string()
            }
        );
        assert_eq!(
            resolve(false, not_working(), Some(&runner(RunnerState::Running))),
            AgentActivity::Queued {
                card_count: 2,
                first_card_title: "Fix login".to_string()
            }
        );
        assert_eq!(
            resolve(false, Facts::default(), None),
            AgentActivity::Idle {
                room_id: None,
                room_title: None,
                last_spoke_at: None
            }
        );
    }

    #[test]
    fn card_state_prefers_working_over_a_pending_card_wake() {
        assert_eq!(card_state(true, true), Some(CardAgentState::Working));
        assert_eq!(card_state(false, true), Some(CardAgentState::Queued));
        assert_eq!(card_state(false, false), None);
    }
}
