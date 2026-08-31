use std::fmt::Write as _;

use sqlx::{FromRow, PgPool};
use uuid::Uuid;

use crate::protocol::{TriagePayload, TriageReportRequest, TriageVerdict};

use super::{auth::AgentClaims, computers::authorize_agent_transaction};

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
        let (system_prompt, role, bio, model) = self.agent_profile(claims).await?;
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
        let mut input = format!(
            "Agent persona:\nrole: {}\nbio: {}\nsystem prompt: {}\n",
            role.as_deref().unwrap_or("unspecified"),
            bio.as_deref().unwrap_or("unspecified"),
            system_prompt,
        );
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
                "This unread delivery is agent-only. Decide whether it needs a full Agent turn. A specific request for this Agent's decision or action is actionable. If recent context shows a human is still waiting and the unread agent message advances that work, it is actionable. Pure acknowledgements, agreement, repetition, and open-ended agent chatter without concrete work are not actionable. When unsure, prefer actionable. Return only JSON with: {\"actionable\": boolean, \"reason\": string, \"promptNote\": string}. Do not answer the message and do not call tools."
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
    ) -> Result<(String, Option<String>, Option<String>, String), sqlx::Error> {
        sqlx::query_as(
            "SELECT a.system_prompt, a.role, a.bio,
                    COALESCE(a.fast_model, a.model) AS fast_model
             FROM collab_agents a
             JOIN collab_computers c ON c.id = a.computer_id
             WHERE a.id = $1 AND a.enabled AND c.daemon_generation = $2
               AND c.status = 'online'",
        )
        .bind(&claims.sub)
        .bind(claims.generation)
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
                    author.display_name AS author_name, m.kind AS message_kind, m.body
             FROM collab_runs r
             JOIN collab_run_deliveries d ON d.run_id = r.id
             JOIN collab_messages m ON m.room_id = d.room_id
                AND m.sequence BETWEEN d.from_seq AND d.up_to_seq
             JOIN collab_rooms room ON room.id = m.room_id
             JOIN collab_participants author ON author.id = m.author_id
             WHERE r.id = $1 AND r.agent_id = $2
               AND r.computer_generation = $3 AND r.status = 'running'
               AND m.author_id <> $2
             ORDER BY m.room_id, m.sequence",
        )
        .bind(run_id)
        .bind(&claims.sub)
        .bind(claims.generation)
        .fetch_all(&self.pool)
        .await?;
        if unread.is_empty() {
            return Err(sqlx::Error::RowNotFound);
        }
        let recent = sqlx::query_as::<_, TriageMessage>(
            "SELECT message.id, message.room_id, room.kind AS room_kind,
                    message.sequence, message.author_id,
                    author.kind AS author_kind, author.display_name AS author_name,
                    message.kind AS message_kind, message.body
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
               AND run.computer_generation = $3 AND run.status = 'running'
             ORDER BY message.room_id, message.sequence",
        )
        .bind(run_id)
        .bind(&claims.sub)
        .bind(claims.generation)
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
            "local_model" | "deterministic" | "system_only"
        ) {
            return Err(protocol_error("INVALID_ARGUMENT: invalid triage source"));
        }
        if request.verdict.source == "system_only" && request.verdict.actionable {
            return Err(protocol_error(
                "INVALID_ARGUMENT: system-only triage cannot be actionable",
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
        let deliveries: Vec<(String, i64)> = sqlx::query_as(
            "SELECT d.room_id, d.up_to_seq
             FROM collab_run_deliveries d
             JOIN collab_runs r ON r.id = d.run_id
             WHERE d.run_id = $1 AND r.agent_id = $2
               AND r.computer_generation = $3 AND r.status = 'running'
             FOR UPDATE OF d",
        )
        .bind(&request.run_id)
        .bind(&claims.sub)
        .bind(claims.generation)
        .fetch_all(&mut *transaction)
        .await?;
        if deliveries.is_empty() {
            return Err(sqlx::Error::RowNotFound);
        }
        for (room_id, up_to_seq) in deliveries {
            sqlx::query(
                "INSERT INTO collab_triages (
                    id, run_id, agent_id, computer_id, room_id, up_to_seq,
                    actionable, source, reason, prompt_note, engine_id, model,
                    input_tokens, output_tokens, latency_ms
                 )
                 SELECT $1, $2, $3, 'local', $4, $5, $6, $7,
                        $8, $9, 'opencode', $10, $11, $12, $13
                 WHERE NOT EXISTS (
                    SELECT 1 FROM collab_triages
                    WHERE run_id = $2 AND room_id = $4
                 )",
            )
            .bind(format!("triage_{}", Uuid::new_v4().simple()))
            .bind(&request.run_id)
            .bind(&claims.sub)
            .bind(&room_id)
            .bind(up_to_seq)
            .bind(request.verdict.actionable)
            .bind(&request.verdict.source)
            .bind(&request.verdict.reason)
            .bind(&request.verdict.prompt_note)
            .bind(&request.model)
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
}

fn append_message(input: &mut String, message: &TriageMessage) {
    let _ = writeln!(
        input,
        "room_id: {}\nroom_kind: {}\nmessage_id: {}\nmessage_kind: {}\nsequence: {}\nauthor_id: {}\nauthor_kind: {}\nauthor_name: {}\nbody: {}\n",
        message.room_id,
        message.room_kind,
        message.id,
        message.message_kind,
        message.sequence,
        message.author_id,
        message.author_kind,
        message.author_name,
        message.body,
    );
}

fn protocol_error(message: &str) -> sqlx::Error {
    sqlx::Error::Protocol(message.to_string())
}
