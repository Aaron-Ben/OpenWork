//! PostgreSQL storage for multi-Agent coordination, triage, and reactions.

use time::PrimitiveDateTime;
use uuid::Uuid;

use super::{CollabStorage, MESSAGE_DEDUP_WINDOW, MessageRow, StorageError};
use crate::{
    coordination::{HeldDecision, held_precheck},
    model::{
        Agent, AgentReplyOutcome, HeldReply, Reaction, RoomGlance, SendMessageOutcome,
        TriageRecord, TriageRecordInput, TriageSettings,
    },
    time::{china_now, format_china},
};

impl CollabStorage {
    pub async fn candidate_agents(
        &self,
        room_id: &str,
        author_id: &str,
    ) -> Result<Vec<Agent>, StorageError> {
        Ok(sqlx::query_as::<_, super::AgentRow>(
            "SELECT a.id, p.display_name, a.role, a.bio, a.system_prompt,
                    a.provider_id, a.model_id, a.opencode_session_id, a.enabled,
                    a.scanner_enabled
               FROM collab_room_members rm
               JOIN collab_agents a ON a.id = rm.participant_id
               JOIN collab_participants p ON p.id = a.id
              WHERE rm.room_id = $1 AND a.enabled = TRUE AND rm.muted = FALSE
                AND a.id <> $2
              ORDER BY a.id",
        )
        .bind(room_id)
        .bind(author_id)
        .fetch_all(&self.pool)
        .await?
        .into_iter()
        .map(super::AgentRow::into_agent)
        .collect())
    }

    pub async fn glance(&self, room_id: &str, limit: u32) -> Result<RoomGlance, StorageError> {
        if !(1..=100).contains(&limit) {
            return Err(StorageError::InvalidInput(
                "glance limit must be between 1 and 100".to_string(),
            ));
        }
        let highest_sequence =
            sqlx::query_scalar::<_, i64>("SELECT next_seq FROM collab_rooms WHERE id = $1")
                .bind(room_id)
                .fetch_optional(&self.pool)
                .await?
                .ok_or_else(|| StorageError::NotFound(format!("room {room_id}")))?;
        let records = sqlx::query_as::<_, MessageRow>(
            "SELECT id, room_id, sequence, author_id, kind, body, system_payload, created_at
               FROM (
                    SELECT id, room_id, sequence, author_id, kind, body, system_payload, created_at
                      FROM collab_messages WHERE room_id = $1
                      ORDER BY sequence DESC LIMIT $2
               ) recent ORDER BY sequence",
        )
        .bind(room_id)
        .bind(i64::from(limit))
        .fetch_all(&self.pool)
        .await?;
        Ok(RoomGlance {
            room_id: room_id.to_string(),
            highest_sequence,
            messages: super::records_to_messages(records)?,
        })
    }

    pub async fn add_reaction(
        &self,
        message_id: &str,
        actor_id: &str,
        emoji: &str,
    ) -> Result<Reaction, StorageError> {
        if emoji.trim().is_empty() {
            return Err(StorageError::InvalidInput(
                "reaction emoji must not be blank".to_string(),
            ));
        }
        let row = sqlx::query_as::<_, ReactionRow>(
            "INSERT INTO collab_reactions (message_id, actor_id, emoji)
             SELECT m.id, $2, $3 FROM collab_messages m
             JOIN collab_room_members rm
               ON rm.room_id = m.room_id AND rm.participant_id = $2
             WHERE m.id = $1
             ON CONFLICT (message_id, actor_id, emoji) DO UPDATE SET emoji = EXCLUDED.emoji
             RETURNING message_id, actor_id, emoji, created_at",
        )
        .bind(message_id)
        .bind(actor_id)
        .bind(emoji.trim())
        .fetch_optional(&self.pool)
        .await?
        .ok_or_else(|| {
            StorageError::NotFound(format!(
                "message {message_id} visible to participant {actor_id}"
            ))
        })?;
        row.try_into_reaction()
    }

    pub async fn configure_triage(
        &self,
        provider_id: &str,
        model_id: &str,
    ) -> Result<TriageSettings, StorageError> {
        let valid: bool = sqlx::query_scalar(
            "SELECT EXISTS(
                SELECT 1 FROM provider_credentials p
                JOIN models m ON m.credential_ref = 'provider:' || p.provider_id
                WHERE p.provider_id = $1 AND p.enabled = TRUE
                  AND m.model_name = $2 AND m.enabled = TRUE
             )",
        )
        .bind(provider_id)
        .bind(model_id)
        .fetch_one(&self.pool)
        .await?;
        if !valid {
            return Err(StorageError::InvalidInput(format!(
                "enabled triage model {provider_id}/{model_id} was not found"
            )));
        }
        sqlx::query(
            "UPDATE collab_settings
                SET triage_provider_id = $1, triage_model_id = $2,
                    updated_at = CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai'
              WHERE id = 'singleton'",
        )
        .bind(provider_id)
        .bind(model_id)
        .execute(&self.pool)
        .await?;
        Ok(TriageSettings {
            provider_id: provider_id.to_string(),
            model_id: model_id.to_string(),
        })
    }

    pub async fn triage_settings(&self) -> Result<Option<TriageSettings>, StorageError> {
        Ok(sqlx::query_as::<_, TriageSettingsRow>(
            "SELECT triage_provider_id, triage_model_id FROM collab_settings
              WHERE id = 'singleton'",
        )
        .fetch_one(&self.pool)
        .await?
        .into_settings())
    }

    pub async fn record_triage(
        &self,
        input: TriageRecordInput<'_>,
    ) -> Result<TriageRecord, StorageError> {
        let id = format!("tri_{}", Uuid::new_v4().simple());
        TriageRecordRow::try_into_record(
            sqlx::query_as::<_, TriageRecordRow>(
                "INSERT INTO collab_triages (
                    id, agent_id, room_id, up_to_seq, actionable, response_mode,
                    source, reason, prompt_note, provider_id, model_id,
                    input_tokens, output_tokens, latency_ms
                 ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14)
                 RETURNING id, agent_id, room_id, up_to_seq, actionable, response_mode,
                    source, reason, prompt_note, provider_id, model_id,
                    input_tokens, output_tokens, latency_ms, created_at",
            )
            .bind(&id)
            .bind(input.agent_id)
            .bind(input.room_id)
            .bind(input.up_to_sequence)
            .bind(input.actionable)
            .bind(input.response_mode)
            .bind(input.source)
            .bind(input.reason)
            .bind(input.prompt_note)
            .bind(input.provider_id)
            .bind(input.model_id)
            .bind(input.input_tokens)
            .bind(input.output_tokens)
            .bind(input.latency_ms)
            .fetch_one(&self.pool)
            .await?,
        )
    }

    pub async fn triage_records(
        &self,
        room_id: Option<&str>,
    ) -> Result<Vec<TriageRecord>, StorageError> {
        let rows = sqlx::query_as::<_, TriageRecordRow>(
            "SELECT id, agent_id, room_id, up_to_seq, actionable, response_mode,
                    source, reason, prompt_note, provider_id, model_id,
                    input_tokens, output_tokens, latency_ms, created_at
               FROM collab_triages
              WHERE $1::TEXT IS NULL OR room_id = $1
              ORDER BY created_at DESC, id DESC LIMIT 200",
        )
        .bind(room_id)
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter()
            .map(TriageRecordRow::try_into_record)
            .collect()
    }

    pub async fn send_agent_reply(
        &self,
        room_id: &str,
        agent_id: &str,
        body: &str,
        seen_sequence: Option<i64>,
    ) -> Result<AgentReplyOutcome, StorageError> {
        if body.trim().is_empty() {
            return Err(StorageError::InvalidInput(
                "message body must not be blank".to_string(),
            ));
        }
        let mut transaction = self.pool.begin().await?;
        let sequence = sqlx::query_scalar::<_, i64>(
            "UPDATE collab_rooms
                SET next_seq = next_seq + 1,
                    updated_at = CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai'
              WHERE id = $1 AND next_seq < 9223372036854775807
              RETURNING next_seq",
        )
        .bind(room_id)
        .fetch_optional(&mut *transaction)
        .await?
        .ok_or_else(|| StorageError::NotFoundOrExhausted(format!("room {room_id}")))?;
        let member_count: i64 =
            sqlx::query_scalar("SELECT count(*) FROM collab_room_members WHERE room_id = $1")
                .bind(room_id)
                .fetch_one(&mut *transaction)
                .await?;
        let peer_sequence = sqlx::query_scalar::<_, Option<i64>>(
            "SELECT MAX(sequence) FROM collab_messages
              WHERE room_id = $1 AND author_id <> $2",
        )
        .bind(room_id)
        .bind(agent_id)
        .fetch_one(&mut *transaction)
        .await?
        .unwrap_or(0);
        let member_count = usize::try_from(member_count)
            .map_err(|_| StorageError::InvalidInput("room member count overflow".to_string()))?;
        if held_precheck(member_count, peer_sequence, seen_sequence) == HeldDecision::Hold {
            let messages = super::records_to_messages(
                sqlx::query_as::<_, MessageRow>(
                    "SELECT id, room_id, sequence, author_id, kind, body, system_payload, created_at
                       FROM collab_messages
                      WHERE room_id = $1 AND author_id <> $2 AND sequence > $3
                      ORDER BY sequence",
                )
                .bind(room_id)
                .bind(agent_id)
                .bind(seen_sequence.unwrap_or(0))
                .fetch_all(&mut *transaction)
                .await?,
            )?;
            transaction.rollback().await?;
            return Ok(AgentReplyOutcome::Held(HeldReply {
                peer_sequence,
                messages,
            }));
        }

        let dedup_window_ms = i64::try_from(MESSAGE_DEDUP_WINDOW.as_millis())
            .map_err(|_| StorageError::InvalidInput("dedup window is too large".to_string()))?;
        if let Some(record) = sqlx::query_as::<_, MessageRow>(
            "SELECT id, room_id, sequence, author_id, kind, body, system_payload, created_at
               FROM collab_messages
              WHERE room_id = $1 AND author_id = $2 AND body = $3
                AND created_at >= (CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai')
                    - ($4::BIGINT * INTERVAL '1 millisecond')
              ORDER BY sequence DESC LIMIT 1",
        )
        .bind(room_id)
        .bind(agent_id)
        .bind(body)
        .bind(dedup_window_ms)
        .fetch_optional(&mut *transaction)
        .await?
        {
            transaction.rollback().await?;
            return Ok(AgentReplyOutcome::Published(SendMessageOutcome {
                message: record.try_into_message()?,
                deduplicated: true,
            }));
        }

        let id = format!("msg_{}", Uuid::new_v4().simple());
        let created_at = china_now();
        let record = sqlx::query_as::<_, MessageRow>(
            "INSERT INTO collab_messages (
                id, room_id, sequence, author_id, kind, body, created_at
             ) VALUES ($1, $2, $3, $4, 'normal', $5, $6)
             RETURNING id, room_id, sequence, author_id, kind, body, system_payload, created_at",
        )
        .bind(&id)
        .bind(room_id)
        .bind(sequence)
        .bind(agent_id)
        .bind(body)
        .bind(created_at)
        .fetch_one(&mut *transaction)
        .await?;
        sqlx::query("UPDATE collab_rooms SET last_message_at = $2, updated_at = $2 WHERE id = $1")
            .bind(room_id)
            .bind(created_at)
            .execute(&mut *transaction)
            .await?;
        transaction.commit().await?;
        Ok(AgentReplyOutcome::Published(SendMessageOutcome {
            message: record.try_into_message()?,
            deduplicated: false,
        }))
    }
}

#[derive(Debug, sqlx::FromRow)]
struct ReactionRow {
    message_id: String,
    actor_id: String,
    emoji: String,
    created_at: PrimitiveDateTime,
}

impl ReactionRow {
    fn try_into_reaction(self) -> Result<Reaction, StorageError> {
        Ok(Reaction {
            message_id: self.message_id,
            actor_id: self.actor_id,
            emoji: self.emoji,
            created_at: format_china(self.created_at)?,
        })
    }
}

#[derive(Debug, sqlx::FromRow)]
struct TriageSettingsRow {
    triage_provider_id: Option<String>,
    triage_model_id: Option<String>,
}

impl TriageSettingsRow {
    fn into_settings(self) -> Option<TriageSettings> {
        Some(TriageSettings {
            provider_id: self.triage_provider_id?,
            model_id: self.triage_model_id?,
        })
    }
}

#[derive(Debug, sqlx::FromRow)]
struct TriageRecordRow {
    id: String,
    agent_id: String,
    room_id: String,
    up_to_seq: i64,
    actionable: bool,
    response_mode: Option<String>,
    source: String,
    reason: Option<String>,
    prompt_note: Option<String>,
    provider_id: Option<String>,
    model_id: Option<String>,
    input_tokens: Option<i64>,
    output_tokens: Option<i64>,
    latency_ms: i64,
    created_at: PrimitiveDateTime,
}

impl TriageRecordRow {
    fn try_into_record(self) -> Result<TriageRecord, StorageError> {
        Ok(TriageRecord {
            id: self.id,
            agent_id: self.agent_id,
            room_id: self.room_id,
            up_to_sequence: self.up_to_seq,
            actionable: self.actionable,
            response_mode: self.response_mode,
            source: self.source,
            reason: self.reason,
            prompt_note: self.prompt_note,
            provider_id: self.provider_id,
            model_id: self.model_id,
            input_tokens: self.input_tokens,
            output_tokens: self.output_tokens,
            latency_ms: self.latency_ms,
            created_at: format_china(self.created_at)?,
        })
    }
}
