use serde_json::{Value, json};
use time::PrimitiveDateTime;
use uuid::Uuid;

use crate::{
    model::{CollabLogEntry, ObservationInput, ObservationRecord},
    observation::TokenUsage,
    time::format_china,
};

use super::{CollabStorage, StorageError};

impl CollabStorage {
    pub async fn update_run_usage(
        &self,
        run_id: &str,
        usage: TokenUsage,
    ) -> Result<(), StorageError> {
        sqlx::query(
            "UPDATE collab_runs
                SET input_tokens = $2,
                    cached_input_tokens = $3,
                    output_tokens = $4
              WHERE id = $1",
        )
        .bind(run_id)
        .bind(usage.input_tokens)
        .bind(usage.cached_input_tokens)
        .bind(usage.output_tokens)
        .execute(self.pool())
        .await?;
        Ok(())
    }

    pub async fn insert_observation(
        &self,
        input: ObservationInput<'_>,
    ) -> Result<ObservationRecord, StorageError> {
        if input.kind.trim().is_empty() {
            return Err(StorageError::InvalidInput(
                "observation kind must not be blank".to_string(),
            ));
        }
        if !input.payload.is_object() {
            return Err(StorageError::InvalidInput(
                "observation payload must be a JSON object".to_string(),
            ));
        }
        ObservationRow::try_into_record(
            sqlx::query_as::<_, ObservationRow>(
                "INSERT INTO collab_events (
                    id, run_id, agent_id, room_id, kind, payload
                 ) VALUES ($1, $2, $3, $4, $5, $6)
                 RETURNING id, run_id, agent_id, room_id, kind, payload, created_at",
            )
            .bind(format!("evt_{}", Uuid::new_v4().simple()))
            .bind(input.run_id)
            .bind(input.agent_id)
            .bind(input.room_id)
            .bind(input.kind)
            .bind(input.payload)
            .fetch_one(self.pool())
            .await?,
        )
    }

    pub async fn log_entries(
        &self,
        room_id: Option<&str>,
        limit: u32,
    ) -> Result<Vec<CollabLogEntry>, StorageError> {
        if !(1..=500).contains(&limit) {
            return Err(StorageError::InvalidInput(
                "log limit must be between 1 and 500".to_string(),
            ));
        }
        let limit = i64::from(limit);
        let mut entries = Vec::new();
        for row in sqlx::query_as::<_, ObservationRow>(
            "SELECT id, run_id, agent_id, room_id, kind, payload, created_at
               FROM collab_events
              WHERE $1::TEXT IS NULL OR room_id = $1
              ORDER BY created_at DESC, id DESC LIMIT $2",
        )
        .bind(room_id)
        .bind(limit)
        .fetch_all(self.pool())
        .await?
        {
            entries.push(TimedLogEntry {
                at: row.created_at,
                entry: CollabLogEntry {
                    source: "event".to_string(),
                    id: row.id,
                    run_id: row.run_id,
                    agent_id: row.agent_id,
                    room_id: row.room_id,
                    kind: row.kind,
                    payload: row.payload,
                    created_at: format_china(row.created_at)?,
                },
            });
        }
        for row in sqlx::query_as::<_, LogRunRow>(
            "SELECT id, agent_id, room_id, trigger, status, started_at, ended_at,
                    provider_id, model_id, input_tokens, cached_input_tokens,
                    output_tokens, error_code, error_message
               FROM collab_runs
              WHERE $1::TEXT IS NULL OR room_id = $1
              ORDER BY started_at DESC, id DESC LIMIT $2",
        )
        .bind(room_id)
        .bind(limit)
        .fetch_all(self.pool())
        .await?
        {
            let ended_at = row.ended_at.map(format_china).transpose()?;
            entries.push(TimedLogEntry {
                at: row.started_at,
                entry: CollabLogEntry {
                    source: "run".to_string(),
                    id: row.id.clone(),
                    run_id: Some(row.id),
                    agent_id: Some(row.agent_id),
                    room_id: row.room_id,
                    kind: format!("run.{}", row.status),
                    payload: json!({
                        "trigger": row.trigger,
                        "status": row.status,
                        "endedAt": ended_at,
                        "providerId": row.provider_id,
                        "modelId": row.model_id,
                        "inputTokens": row.input_tokens,
                        "cachedInputTokens": row.cached_input_tokens,
                        "outputTokens": row.output_tokens,
                        "errorCode": row.error_code,
                        "errorMessage": row.error_message,
                    }),
                    created_at: format_china(row.started_at)?,
                },
            });
        }
        for row in sqlx::query_as::<_, LogTriageRow>(
            "SELECT id, agent_id, room_id, up_to_seq, actionable, response_mode,
                    source, reason, prompt_note, provider_id, model_id,
                    input_tokens, output_tokens, latency_ms, created_at
               FROM collab_triages
              WHERE $1::TEXT IS NULL OR room_id = $1
              ORDER BY created_at DESC, id DESC LIMIT $2",
        )
        .bind(room_id)
        .bind(limit)
        .fetch_all(self.pool())
        .await?
        {
            entries.push(TimedLogEntry {
                at: row.created_at,
                entry: CollabLogEntry {
                    source: "triage".to_string(),
                    id: row.id,
                    run_id: None,
                    agent_id: Some(row.agent_id),
                    room_id: Some(row.room_id),
                    kind: "triage.decision".to_string(),
                    payload: json!({
                        "upToSequence": row.up_to_seq,
                        "actionable": row.actionable,
                        "responseMode": row.response_mode,
                        "source": row.source,
                        "reason": row.reason,
                        "promptNote": row.prompt_note,
                        "providerId": row.provider_id,
                        "modelId": row.model_id,
                        "inputTokens": row.input_tokens,
                        "outputTokens": row.output_tokens,
                        "latencyMs": row.latency_ms,
                    }),
                    created_at: format_china(row.created_at)?,
                },
            });
        }
        entries.sort_by(|left, right| {
            right
                .at
                .cmp(&left.at)
                .then_with(|| right.entry.id.cmp(&left.entry.id))
        });
        entries.truncate(limit as usize);
        Ok(entries.into_iter().map(|item| item.entry).collect())
    }
}

struct TimedLogEntry {
    at: PrimitiveDateTime,
    entry: CollabLogEntry,
}

#[derive(Debug, sqlx::FromRow)]
struct ObservationRow {
    id: String,
    run_id: Option<String>,
    agent_id: Option<String>,
    room_id: Option<String>,
    kind: String,
    payload: Value,
    created_at: PrimitiveDateTime,
}

impl ObservationRow {
    fn try_into_record(self) -> Result<ObservationRecord, StorageError> {
        Ok(ObservationRecord {
            id: self.id,
            run_id: self.run_id,
            agent_id: self.agent_id,
            room_id: self.room_id,
            kind: self.kind,
            payload: self.payload,
            created_at: format_china(self.created_at)?,
        })
    }
}

#[derive(Debug, sqlx::FromRow)]
struct LogRunRow {
    id: String,
    agent_id: String,
    room_id: Option<String>,
    trigger: String,
    status: String,
    started_at: PrimitiveDateTime,
    ended_at: Option<PrimitiveDateTime>,
    provider_id: Option<String>,
    model_id: Option<String>,
    input_tokens: Option<i64>,
    cached_input_tokens: Option<i64>,
    output_tokens: Option<i64>,
    error_code: Option<String>,
    error_message: Option<String>,
}

#[derive(Debug, sqlx::FromRow)]
struct LogTriageRow {
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
