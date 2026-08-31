use std::{
    collections::{BTreeMap, BTreeSet},
    fmt::Write as _,
};

use crate::protocol::{TriagePayload, TriageReportRequest, TriageVerdict, entity_id};
use sqlx::{FromRow, PgPool};

use super::{
    auth::{AgentClaims, authorize_agent_transaction},
    climate::Climate,
};

const AGENT_LOOP_TRIAGE_INTERVAL: i64 = 8;
pub(super) const AGENT_LOOP_HARD_CAP: i64 = 20;

#[derive(Clone)]
pub struct InboxTriage {
    pool: PgPool,
}

impl InboxTriage {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn payload(
        &self,
        claims: &AgentClaims,
        run_id: &str,
    ) -> Result<TriagePayload, sqlx::Error> {
        let (persona, role, model) = self.agent_profile(claims).await?;
        let context = self.context(claims, run_id).await?;
        if context
            .unread
            .iter()
            .all(|message| message.message_kind == "system")
        {
            return Ok(TriagePayload {
                verdict: Some(TriageVerdict {
                    actionable: false,
                    reason: "unread delivery contains only system messages".to_string(),
                    prompt_note: String::new(),
                    source: "system_only".to_string(),
                }),
                instructions: None,
                input: None,
                model,
            });
        }
        if context
            .unread
            .iter()
            .any(|message| message.author_kind == "user")
        {
            return Ok(TriagePayload {
                verdict: Some(TriageVerdict {
                    actionable: true,
                    reason: "unread delivery contains a human message".to_string(),
                    prompt_note: "A human is waiting. Read whom they addressed and respond only if this Agent is the intended teammate or the whole group was addressed.".to_string(),
                    source: "deterministic".to_string(),
                }),
                instructions: None,
                input: None,
                model,
            });
        }
        let real_unread = context
            .unread
            .iter()
            .filter(|message| message.message_kind != "system")
            .collect::<Vec<_>>();
        if let Some(verdict) = agent_loop_verdict(&real_unread) {
            return Ok(TriagePayload {
                verdict: Some(verdict),
                instructions: None,
                input: None,
                model,
            });
        }
        let mut input = format!(
            "Agent persona:\nrole: {}\npersona: {}\n",
            role.as_deref().unwrap_or("unspecified"),
            persona,
        );
        let participant_ids = context
            .unread
            .iter()
            .chain(&context.recent)
            .map(|message| message.author_id.clone())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();
        let climates = Climate::for_participants(&self.pool, &claims.sub, &participant_ids).await?;
        if !climates.is_empty() {
            input.push_str(
                "\nPrivate Climate context (this Agent's subjective current impressions):\n",
            );
            for climate in climates {
                let _ = writeln!(
                    input,
                    "about_participant_id: {}\naffinity: {}\ntrust: {}\nnote: {}\n",
                    climate.about_participant_id,
                    climate.affinity,
                    climate.trust,
                    climate.last_note.as_deref().unwrap_or("none"),
                );
            }
        }
        if !context.recent.is_empty() {
            input.push_str("\nRecent posted context:\n");
            for message in &context.recent {
                append_message(&mut input, message);
            }
        }
        input.push_str("\nUnread durable inbox:\n");
        for message in &context.unread {
            append_message(&mut input, message);
        }
        Ok(TriagePayload {
            verdict: None,
            instructions: Some(
                "This unread delivery is agent-only. Decide whether it needs a full Agent turn. A specific request for this Agent's decision or action is actionable. If recent context shows a human is still waiting and the unread agent message advances that work, it is actionable. Pure acknowledgements, agreement, repetition, and open-ended agent chatter without concrete work are not actionable. A Room with agent_streak 20 or higher is hard capped: acknowledge it instead of replying. When unsure, prefer actionable. Return only JSON with: {\"actionable\": boolean, \"reason\": string, \"promptNote\": string}. Do not answer the message and do not call tools."
                    .to_string(),
            ),
            input: Some(input),
            model,
        })
    }

    pub async fn report(
        &self,
        claims: &AgentClaims,
        request: &TriageReportRequest,
    ) -> Result<(), sqlx::Error> {
        self.record(claims, request).await
    }

    async fn agent_profile(
        &self,
        claims: &AgentClaims,
    ) -> Result<(String, Option<String>, String), sqlx::Error> {
        sqlx::query_as(
            "SELECT profile.persona, profile.role, config.triage_model_id
             FROM collab_agent_profiles profile
             JOIN collab_agent_runtime_configs config ON config.agent_id = profile.agent_id
             WHERE profile.agent_id = $1 AND profile.archived_at IS NULL",
        )
        .bind(&claims.sub)
        .fetch_one(&self.pool)
        .await
    }

    async fn context(
        &self,
        claims: &AgentClaims,
        run_id: &str,
    ) -> Result<TriageContext, sqlx::Error> {
        let unread = sqlx::query_as::<_, TriageMessage>(
            "SELECT m.id, m.room_id, room.kind AS room_kind, m.sequence,
                    m.author_id, author.kind AS author_kind,
                    author.display_name AS author_name, m.kind AS message_kind, m.body,
                    agent_chain.agent_streak
             FROM collab_runs r
             JOIN collab_run_deliveries d ON d.run_id = r.id
             JOIN collab_messages m ON m.room_id = d.room_id
                AND m.sequence BETWEEN d.from_seq AND d.up_to_seq
             JOIN collab_rooms room ON room.id = m.room_id
             JOIN collab_participants author ON author.id = m.author_id
             JOIN LATERAL (
                 SELECT COUNT(*) AS agent_streak
                 FROM collab_messages trailing_message
                 JOIN collab_participants trailing_author
                   ON trailing_author.id = trailing_message.author_id
                 WHERE trailing_message.room_id = d.room_id
                   AND trailing_message.sequence <= d.up_to_seq
                   AND trailing_message.sequence > COALESCE((
                       SELECT MAX(previous.sequence)
                       FROM collab_messages previous
                       JOIN collab_participants previous_author
                         ON previous_author.id = previous.author_id
                       WHERE previous.room_id = d.room_id
                         AND previous.sequence <= d.up_to_seq
                         AND previous_author.kind <> 'agent'
                   ), 0)
                   AND trailing_author.kind = 'agent'
                   AND trailing_message.kind <> 'system'
             ) agent_chain ON TRUE
             WHERE r.id = $1 AND r.agent_id = $2
               AND r.runtime_session_id = $3 AND r.status = 'running'
               AND m.author_id <> $2
             ORDER BY m.room_id, m.sequence",
        )
        .bind(run_id)
        .bind(&claims.sub)
        .bind(&claims.runtime_session_id)
        .fetch_all(&self.pool)
        .await?;
        if unread.is_empty() {
            return Err(sqlx::Error::RowNotFound);
        }
        let recent = sqlx::query_as::<_, TriageMessage>(
            "SELECT message.id, message.room_id, room.kind AS room_kind,
                    message.sequence, message.author_id,
                    author.kind AS author_kind, author.display_name AS author_name,
                    message.kind AS message_kind, message.body,
                    0::BIGINT AS agent_streak
             FROM collab_runs run
             JOIN collab_run_deliveries delivery ON delivery.run_id = run.id
             JOIN collab_room_members member
               ON member.room_id = delivery.room_id
              AND member.participant_id = run.agent_id
             JOIN LATERAL (
                 SELECT history.id, history.room_id, history.sequence,
                        history.author_id, history.kind, history.body
                 FROM collab_messages history
                 WHERE history.room_id = delivery.room_id
                   AND history.sequence < delivery.from_seq
                   AND history.created_at >= member.joined_at
                 ORDER BY history.sequence DESC
                 LIMIT 12
             ) message ON TRUE
             JOIN collab_rooms room ON room.id = message.room_id
             JOIN collab_participants author ON author.id = message.author_id
             WHERE run.id = $1 AND run.agent_id = $2
               AND run.runtime_session_id = $3 AND run.status = 'running'
             ORDER BY message.room_id, message.sequence",
        )
        .bind(run_id)
        .bind(&claims.sub)
        .bind(&claims.runtime_session_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(TriageContext { unread, recent })
    }

    async fn record(
        &self,
        claims: &AgentClaims,
        request: &TriageReportRequest,
    ) -> Result<(), sqlx::Error> {
        let mut transaction = self.pool.begin().await?;
        authorize_agent_transaction(&mut transaction, claims).await?;
        if !matches!(
            request.verdict.source.as_str(),
            "local_model" | "deterministic" | "system_only" | "agent_dm_engage" | "loop_cap"
        ) {
            return Err(protocol_error("INVALID_ARGUMENT: invalid triage source"));
        }
        if matches!(request.verdict.source.as_str(), "system_only" | "loop_cap")
            && request.verdict.actionable
        {
            return Err(protocol_error(
                "INVALID_ARGUMENT: suppressing triage source cannot be actionable",
            ));
        }
        if request.verdict.source == "agent_dm_engage" && !request.verdict.actionable {
            return Err(protocol_error(
                "INVALID_ARGUMENT: Agent DM engage triage must be actionable",
            ));
        }
        let existing: Option<bool> = sqlx::query_scalar(
            "SELECT actionable FROM collab_triages
             WHERE run_id = $1
             LIMIT 1",
        )
        .bind(&request.run_id)
        .fetch_optional(&mut *transaction)
        .await?;
        if existing.is_some_and(|actionable| actionable != request.verdict.actionable) {
            return Err(protocol_error(
                "CONFLICT: triage verdict is immutable once recorded",
            ));
        }
        let deliveries: Vec<(String, i64, String, String)> = sqlx::query_as(
            "SELECT d.room_id, d.up_to_seq, r.engine_id, r.triage_model_id
             FROM collab_run_deliveries d
             JOIN collab_runs r ON r.id = d.run_id
             WHERE d.run_id = $1 AND r.agent_id = $2
               AND r.runtime_session_id = $3 AND r.status = 'running'
             FOR UPDATE OF d",
        )
        .bind(&request.run_id)
        .bind(&claims.sub)
        .bind(&claims.runtime_session_id)
        .fetch_all(&mut *transaction)
        .await?;
        if deliveries.is_empty() {
            return Err(sqlx::Error::RowNotFound);
        }
        for (room_id, up_to_seq, engine_id, model_id) in deliveries {
            sqlx::query(
                "INSERT INTO collab_triages (
                    id, run_id, agent_id, runtime_session_id, room_id, up_to_seq,
                    actionable, source, reason, prompt_note, engine_id, model_id,
                    input_tokens, output_tokens, latency_ms
                 )
                 SELECT $1, $2, $3, $4, $5, $6, $7, $8,
                        $9, $10, $11, $12, $13, $14, $15
                 WHERE NOT EXISTS (
                    SELECT 1 FROM collab_triages
                    WHERE run_id = $2 AND room_id = $5
                 )",
            )
            .bind(entity_id("triage"))
            .bind(&request.run_id)
            .bind(&claims.sub)
            .bind(&claims.runtime_session_id)
            .bind(&room_id)
            .bind(up_to_seq)
            .bind(request.verdict.actionable)
            .bind(&request.verdict.source)
            .bind(&request.verdict.reason)
            .bind(&request.verdict.prompt_note)
            .bind(&engine_id)
            .bind(if request.model.trim().is_empty() {
                &model_id
            } else {
                &request.model
            })
            .bind(request.input_tokens)
            .bind(request.output_tokens)
            .bind(request.latency_ms)
            .execute(&mut *transaction)
            .await?;
            if !request.verdict.actionable {
                sqlx::query(
                    "UPDATE collab_run_deliveries
                     SET eligible_reason = 'triage_false',
                         eligible_at = CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai'
                     WHERE run_id = $1 AND room_id = $2 AND eligible_reason IS NULL",
                )
                .bind(&request.run_id)
                .bind(&room_id)
                .execute(&mut *transaction)
                .await?;
            }
        }
        transaction.commit().await
    }
}

fn agent_loop_verdict(messages: &[&TriageMessage]) -> Option<TriageVerdict> {
    if messages.is_empty()
        || !messages
            .iter()
            .all(|message| message.author_kind == "agent")
    {
        return None;
    }
    let mut latest_by_room = BTreeMap::<&str, (&str, i64)>::new();
    for message in messages {
        latest_by_room
            .entry(&message.room_id)
            .and_modify(|(_, streak)| *streak = (*streak).max(message.agent_streak))
            .or_insert((&message.room_kind, message.agent_streak));
    }
    if latest_by_room
        .values()
        .all(|(_, streak)| *streak >= AGENT_LOOP_HARD_CAP)
    {
        return Some(TriageVerdict {
            actionable: false,
            reason: format!(
                "Agent conversation reached the {AGENT_LOOP_HARD_CAP}-message hard loop cap"
            ),
            prompt_note: String::new(),
            source: "loop_cap".to_string(),
        });
    }
    if latest_by_room.values().all(|(room_kind, streak)| {
        *room_kind == "direct" && *streak % AGENT_LOOP_TRIAGE_INTERVAL != 0
    }) {
        return Some(TriageVerdict {
            actionable: true,
            reason: format!(
                "Agent-to-Agent Direct Room engages between every {AGENT_LOOP_TRIAGE_INTERVAL}th-message loop check"
            ),
            prompt_note: "A teammate messaged you directly. Reply in that Direct Room.".to_string(),
            source: "agent_dm_engage".to_string(),
        });
    }
    None
}

struct TriageContext {
    unread: Vec<TriageMessage>,
    recent: Vec<TriageMessage>,
}

#[derive(FromRow)]
struct TriageMessage {
    id: String,
    room_id: String,
    room_kind: String,
    sequence: i64,
    author_id: String,
    author_kind: String,
    author_name: String,
    message_kind: String,
    body: String,
    agent_streak: i64,
}

fn append_message(input: &mut String, message: &TriageMessage) {
    let _ = writeln!(
        input,
        "room_id: {}\nroom_kind: {}\nmessage_id: {}\nmessage_kind: {}\nsequence: {}\nagent_streak: {}\nauthor_id: {}\nauthor_kind: {}\nauthor_name: {}\nbody: {}\n",
        message.room_id,
        message.room_kind,
        message.id,
        message.message_kind,
        message.sequence,
        message.agent_streak,
        message.author_id,
        message.author_kind,
        message.author_name,
        message.body,
    );
}

fn protocol_error(message: &str) -> sqlx::Error {
    sqlx::Error::Protocol(message.to_string())
}

#[cfg(test)]
mod tests {
    use super::{TriageMessage, agent_loop_verdict};

    fn agent_message(room_id: &str, room_kind: &str, agent_streak: i64) -> TriageMessage {
        TriageMessage {
            id: format!("msg-{agent_streak}"),
            room_id: room_id.to_string(),
            room_kind: room_kind.to_string(),
            sequence: agent_streak,
            author_id: "agent-peer".to_string(),
            author_kind: "agent".to_string(),
            author_name: "Peer".to_string(),
            message_kind: "normal".to_string(),
            body: "hello".to_string(),
            agent_streak,
        }
    }

    #[test]
    fn direct_check_cadence_and_hard_cap_are_independent() {
        let between = agent_message("room-direct", "direct", 7);
        let verdict = agent_loop_verdict(&[&between]).expect("between checks engages");
        assert!(verdict.actionable);
        assert_eq!(verdict.source, "agent_dm_engage");

        let checkpoint = agent_message("room-direct", "direct", 8);
        assert!(agent_loop_verdict(&[&checkpoint]).is_none());

        let capped = agent_message("room-direct", "direct", 20);
        let verdict = agent_loop_verdict(&[&capped]).expect("hard cap suppresses");
        assert!(!verdict.actionable);
        assert_eq!(verdict.source, "loop_cap");
    }

    #[test]
    fn group_uses_model_triage_until_the_deterministic_agent_loop_cap() {
        let slow_group = agent_message("room-group", "group", 8);
        assert!(agent_loop_verdict(&[&slow_group]).is_none());

        let capped_group = agent_message("room-group", "group", 20);
        let verdict = agent_loop_verdict(&[&capped_group]).expect("group cap suppresses");
        assert!(!verdict.actionable);
        assert_eq!(verdict.source, "loop_cap");

        let fresh_other_room = agent_message("room-other", "group", 1);
        assert!(agent_loop_verdict(&[&capped_group, &fresh_other_room]).is_none());
    }
}
