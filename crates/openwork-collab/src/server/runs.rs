use sqlx::{FromRow, PgPool};

use crate::protocol::{FinishRunRequest, RunSummaryView, RunView, TriggerEnvelope};

use super::{auth::AgentClaims, computers::authorize_agent_transaction};

#[derive(Clone)]
pub(crate) struct Runs {
    pool: PgPool,
}

impl Runs {
    pub(crate) fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub(crate) async fn list(&self, limit: u32) -> Result<Vec<RunSummaryView>, sqlx::Error> {
        let limit = i64::from(limit.clamp(1, 200));
        sqlx::query_as::<_, RunSummaryRow>(
            "SELECT id, agent_id, trigger, status, model, outcome, room_id, focus_card_id,
                    trigger_reason, error_code, error_message,
                    to_char(started_at, 'YYYY-MM-DD\"T\"HH24:MI:SS') || '+08:00' AS started_at
             FROM collab_runs ORDER BY started_at DESC, id DESC LIMIT $1",
        )
        .bind(limit)
        .fetch_all(&self.pool)
        .await
        .map(|rows| rows.into_iter().map(RunSummaryView::from).collect())
    }

    pub(crate) async fn open(
        &self,
        claims: &AgentClaims,
        trigger: &TriggerEnvelope,
    ) -> Result<RunView, sqlx::Error> {
        let mut transaction = self.pool.begin().await?;
        authorize_agent_transaction(&mut transaction, claims).await?;
        let focus = trigger.agenda_focus.as_ref();
        if let Some(focus) = focus {
            let current: bool = sqlx::query_scalar(
                "SELECT EXISTS(
                    SELECT 1
                    FROM collab_rooms room
                    JOIN collab_room_members member
                      ON member.room_id = room.id AND member.participant_id = $2
                    WHERE room.id = $1 AND room.next_seq = $3
                      AND (
                        $4::TEXT IS NULL OR EXISTS (
                            SELECT 1
                            FROM collab_cards card
                            JOIN collab_boards board ON board.id = card.board_id
                            JOIN collab_board_columns board_column
                              ON board_column.id = card.column_id AND NOT board_column.is_done
                            WHERE card.id = $4 AND board.room_id = room.id
                              AND (
                                card.claimed_by = $2 OR
                                (card.claimed_by IS NULL AND card.assignee_id = $2)
                              )
                        )
                      )
                 )",
            )
            .bind(&focus.room_id)
            .bind(&claims.sub)
            .bind(focus.room_sequence)
            .bind(&focus.card_id)
            .fetch_one(&mut *transaction)
            .await?;
            if !current {
                return Err(protocol_error(
                    "CONFLICT: agenda focus changed before the run opened",
                ));
            }
        }
        let room_id = focus.map(|focus| focus.room_id.as_str()).or_else(|| {
            trigger
                .deliveries
                .first()
                .map(|delivery| delivery.room_id.as_str())
        });
        sqlx::query(
            "INSERT INTO collab_runs (
                id, agent_id, computer_id, room_id, trigger, status,
                engine_id, model, computer_generation, inbox_carried_over,
                focus_card_id, agenda_anchor_seq, trigger_reason
             )
             SELECT $1, a.id, a.computer_id, $3, $4, 'running',
                    a.engine_id, a.model, $5, $6, $7, $8, $9
             FROM collab_agents a
             WHERE a.id = $2
             ON CONFLICT (id) DO NOTHING",
        )
        .bind(&trigger.dispatch_id)
        .bind(&claims.sub)
        .bind(room_id)
        .bind(&trigger.trigger)
        .bind(claims.generation)
        .bind(trigger.carried_over)
        .bind(focus.and_then(|focus| focus.card_id.as_deref()))
        .bind(focus.map(|focus| focus.room_sequence))
        .bind(focus.map(|focus| focus.reason.as_str()))
        .execute(&mut *transaction)
        .await?;
        for delivery in &trigger.deliveries {
            let member: bool = sqlx::query_scalar(
                "SELECT EXISTS(
                    SELECT 1 FROM collab_room_members
                    WHERE room_id = $1 AND participant_id = $2
                 )",
            )
            .bind(&delivery.room_id)
            .bind(&claims.sub)
            .fetch_one(&mut *transaction)
            .await?;
            if !member {
                return Err(sqlx::Error::RowNotFound);
            }
            sqlx::query(
                "INSERT INTO collab_run_deliveries (run_id, room_id, from_seq, up_to_seq)
                 VALUES ($1, $2, $3, $4)
                 ON CONFLICT (run_id, room_id) DO NOTHING",
            )
            .bind(&trigger.dispatch_id)
            .bind(&delivery.room_id)
            .bind(delivery.from_seq)
            .bind(delivery.up_to_seq)
            .execute(&mut *transaction)
            .await?;
        }
        let status: String =
            sqlx::query_scalar("SELECT status FROM collab_runs WHERE id = $1 AND agent_id = $2")
                .bind(&trigger.dispatch_id)
                .bind(&claims.sub)
                .fetch_one(&mut *transaction)
                .await?;
        transaction.commit().await?;
        Ok(RunView {
            id: trigger.dispatch_id.clone(),
            status,
            outcome: None,
        })
    }

    pub(crate) async fn heartbeat(
        &self,
        claims: &AgentClaims,
        run_id: &str,
    ) -> Result<(), sqlx::Error> {
        let mut transaction = self.pool.begin().await?;
        authorize_agent_transaction(&mut transaction, claims).await?;
        let result = sqlx::query(
            "UPDATE collab_runs
             SET heartbeat_at = CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai'
             WHERE id = $1 AND agent_id = $2 AND computer_generation = $3
               AND status = 'running'",
        )
        .bind(run_id)
        .bind(&claims.sub)
        .bind(claims.generation)
        .execute(&mut *transaction)
        .await?;
        if result.rows_affected() == 0 {
            return Err(sqlx::Error::RowNotFound);
        }
        transaction.commit().await
    }

    pub(crate) async fn finish(
        &self,
        claims: &AgentClaims,
        run_id: &str,
        request: FinishRunRequest,
    ) -> Result<RunView, sqlx::Error> {
        let terminal_status = match request.status.as_str() {
            "completed" | "failed" | "cancelled" | "interrupted" => request.status.as_str(),
            _ => return Err(protocol_error("INVALID_ARGUMENT: invalid run status")),
        };
        let mut transaction = self.pool.begin().await?;
        authorize_agent_transaction(&mut transaction, claims).await?;
        let (current_status, current_outcome): (String, Option<String>) = sqlx::query_as(
            "SELECT status, outcome FROM collab_runs
             WHERE id = $1 AND agent_id = $2 AND computer_generation = $3
             FOR UPDATE",
        )
        .bind(run_id)
        .bind(&claims.sub)
        .bind(claims.generation)
        .fetch_one(&mut *transaction)
        .await?;
        if current_status != "running" {
            transaction.commit().await?;
            return Ok(RunView {
                id: run_id.to_string(),
                status: current_status,
                outcome: current_outcome,
            });
        }
        let mut outcome = None;
        if terminal_status == "completed" {
            let deliveries: Vec<(String, i64, Option<String>)> = sqlx::query_as(
                "SELECT room_id, up_to_seq, eligible_reason
                 FROM collab_run_deliveries WHERE run_id = $1",
            )
            .bind(run_id)
            .fetch_all(&mut *transaction)
            .await?;
            let mut acted = false;
            let mut acknowledged = false;
            for (room_id, up_to_seq, reason) in deliveries {
                if let Some(reason) = reason {
                    sqlx::query(
                        "UPDATE collab_room_members
                         SET last_read_seq = GREATEST(last_read_seq, $1)
                         WHERE room_id = $2 AND participant_id = $3",
                    )
                    .bind(up_to_seq)
                    .bind(&room_id)
                    .bind(&claims.sub)
                    .execute(&mut *transaction)
                    .await?;
                    sqlx::query(
                        "UPDATE collab_run_deliveries
                         SET settled_at = CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai'
                         WHERE run_id = $1 AND room_id = $2",
                    )
                    .bind(run_id)
                    .bind(&room_id)
                    .execute(&mut *transaction)
                    .await?;
                    acted |= reason == "action";
                    acknowledged |= reason == "ack" || reason == "triage_false";
                }
            }
            let action_recorded: bool = sqlx::query_scalar(
                "SELECT EXISTS (
                    SELECT 1
                    FROM collab_cli_requests request,
                         LATERAL jsonb_array_elements(
                             COALESCE(request.result -> 'sideEffects', '[]'::jsonb)
                         ) effect
                    WHERE request.run_id = $1
                      AND effect ->> 'type' IN (
                          'message_published', 'reaction_changed',
                          'card_created', 'card_claimed', 'card_moved'
                      )
                 )",
            )
            .bind(run_id)
            .fetch_one(&mut *transaction)
            .await?;
            acted |= action_recorded;
            outcome = Some(if acted {
                "acted"
            } else if acknowledged
                || request
                    .assistant_text
                    .as_deref()
                    .is_none_or(|text| text.trim().is_empty())
            {
                "silent"
            } else {
                "unpublished"
            });
        }
        sqlx::query(
            "UPDATE collab_runs
             SET status = $1, ended_at = CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai',
                 heartbeat_at = CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai',
                 input_tokens = $2, cached_input_tokens = $3, output_tokens = $4,
                 error_code = $5, error_message = $6, outcome = $7
             WHERE id = $8",
        )
        .bind(terminal_status)
        .bind(request.input_tokens)
        .bind(request.cached_input_tokens)
        .bind(request.output_tokens)
        .bind(request.error_code)
        .bind(request.error_message)
        .bind(outcome)
        .bind(run_id)
        .execute(&mut *transaction)
        .await?;
        transaction.commit().await?;
        Ok(RunView {
            id: run_id.to_string(),
            status: terminal_status.to_string(),
            outcome: outcome.map(str::to_string),
        })
    }
}

#[derive(FromRow)]
struct RunSummaryRow {
    id: String,
    agent_id: String,
    trigger: String,
    status: String,
    model: String,
    outcome: Option<String>,
    room_id: Option<String>,
    focus_card_id: Option<String>,
    trigger_reason: Option<String>,
    error_code: Option<String>,
    error_message: Option<String>,
    started_at: String,
}

impl From<RunSummaryRow> for RunSummaryView {
    fn from(row: RunSummaryRow) -> Self {
        Self {
            id: row.id,
            agent_id: row.agent_id,
            trigger: row.trigger,
            status: row.status,
            model: row.model,
            outcome: row.outcome,
            room_id: row.room_id,
            focus_card_id: row.focus_card_id,
            trigger_reason: row.trigger_reason,
            error_code: row.error_code,
            error_message: row.error_message,
            started_at: row.started_at,
        }
    }
}

fn protocol_error(message: &str) -> sqlx::Error {
    sqlx::Error::Protocol(message.to_string())
}
