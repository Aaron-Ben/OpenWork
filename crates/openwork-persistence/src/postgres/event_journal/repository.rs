use async_trait::async_trait;
use openwork_protocol::journal::{
    AggregateType, EventJournal, EventJournalError, ExpectedVersion, NewRecordedEventV1,
    RecordedEventV1,
};
use sqlx::PgPool;

use super::record::RecordedEventRecord;

const EVENT_SELECT: &str = r#"
    global_position,
    event_id,
    aggregate_type,
    aggregate_id,
    aggregate_version,
    event_type,
    event_version,
    payload_json,
    (EXTRACT(EPOCH FROM (occurred_at AT TIME ZONE 'Asia/Shanghai')) * 1000)::BIGINT
        AS occurred_at_unix_ms,
    (EXTRACT(EPOCH FROM (recorded_at AT TIME ZONE 'Asia/Shanghai')) * 1000)::BIGINT
        AS recorded_at_unix_ms
"#;

#[derive(Clone)]
pub struct PostgresEventJournal {
    pool: PgPool,
}

impl PostgresEventJournal {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub fn pool(&self) -> &PgPool {
        &self.pool
    }
}

#[async_trait]
impl EventJournal for PostgresEventJournal {
    async fn append(
        &self,
        aggregate_type: AggregateType,
        aggregate_id: &str,
        expected_version: ExpectedVersion,
        events: Vec<NewRecordedEventV1>,
    ) -> Result<Vec<RecordedEventV1>, EventJournalError> {
        validate_append(aggregate_id, &events)?;
        if events.is_empty() {
            return Ok(Vec::new());
        }

        let mut transaction = self.pool.begin().await.map_err(persistence_error)?;

        // A new aggregate has no row that SELECT ... FOR UPDATE could lock.
        // A transaction-scoped advisory lock serializes version allocation for
        // this aggregate without introducing a second aggregate-head table.
        sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1 || chr(31) || $2, 0))")
            .bind(aggregate_type.as_str())
            .bind(aggregate_id)
            .execute(&mut *transaction)
            .await
            .map_err(persistence_error)?;

        let actual_version: i64 = sqlx::query_scalar(
            "SELECT COALESCE(MAX(aggregate_version), 0)::BIGINT
             FROM recorded_events
             WHERE aggregate_type = $1 AND aggregate_id = $2",
        )
        .bind(aggregate_type.as_str())
        .bind(aggregate_id)
        .fetch_one(&mut *transaction)
        .await
        .map_err(persistence_error)?;
        let actual_version =
            u64::try_from(actual_version).map_err(|_| EventJournalError::Persistence {
                message: "aggregate version read from PostgreSQL was negative".to_string(),
            })?;
        let expected = expected_version.expected_current_version();
        if actual_version != expected {
            return Err(EventJournalError::VersionConflict {
                aggregate_type,
                aggregate_id: aggregate_id.to_string(),
                expected,
                actual: actual_version,
            });
        }

        let mut appended = Vec::with_capacity(events.len());
        for (offset, event) in events.into_iter().enumerate() {
            let aggregate_version =
                actual_version
                    .checked_add(offset as u64 + 1)
                    .ok_or_else(|| EventJournalError::InvalidEvent {
                        message: "aggregate version overflow".to_string(),
                    })?;
            let aggregate_version =
                i64::try_from(aggregate_version).map_err(|_| EventJournalError::InvalidEvent {
                    message: "aggregate version exceeds PostgreSQL BIGINT".to_string(),
                })?;
            let event_version = i32::try_from(event.event_version).map_err(|_| {
                EventJournalError::InvalidEvent {
                    message: "event version exceeds PostgreSQL INTEGER".to_string(),
                }
            })?;

            let sql = format!(
                "INSERT INTO recorded_events (
                    event_id, aggregate_type, aggregate_id, aggregate_version,
                    event_type, event_version, payload_json, occurred_at
                 ) VALUES (
                    $1, $2, $3, $4, $5, $6, $7,
                    to_timestamp($8::DOUBLE PRECISION / 1000.0) AT TIME ZONE 'Asia/Shanghai'
                 )
                 RETURNING {EVENT_SELECT}"
            );
            let record = sqlx::query_as::<_, RecordedEventRecord>(&sql)
                .bind(event.event_id.as_str())
                .bind(aggregate_type.as_str())
                .bind(aggregate_id)
                .bind(aggregate_version)
                .bind(&event.event_type)
                .bind(event_version)
                .bind(&event.payload)
                .bind(event.occurred_at_unix_ms)
                .fetch_one(&mut *transaction)
                .await
                .map_err(|error| map_insert_error(error, event.event_id.clone()))?;
            appended.push(record.try_into()?);
        }

        transaction.commit().await.map_err(persistence_error)?;
        Ok(appended)
    }

    async fn load_aggregate(
        &self,
        aggregate_type: AggregateType,
        aggregate_id: &str,
        after_version: u64,
    ) -> Result<Vec<RecordedEventV1>, EventJournalError> {
        if aggregate_id.trim().is_empty() {
            return Err(EventJournalError::InvalidEvent {
                message: "aggregate_id must not be blank".to_string(),
            });
        }
        let after_version =
            i64::try_from(after_version).map_err(|_| EventJournalError::InvalidEvent {
                message: "after_version exceeds PostgreSQL BIGINT".to_string(),
            })?;
        let sql = format!(
            "SELECT {EVENT_SELECT}
             FROM recorded_events
             WHERE aggregate_type = $1
               AND aggregate_id = $2
               AND aggregate_version > $3
             ORDER BY aggregate_version ASC"
        );
        let records = sqlx::query_as::<_, RecordedEventRecord>(&sql)
            .bind(aggregate_type.as_str())
            .bind(aggregate_id)
            .bind(after_version)
            .fetch_all(&self.pool)
            .await
            .map_err(persistence_error)?;
        convert_records(records)
    }

    async fn read_all(
        &self,
        after_global_position: u64,
        limit: u32,
    ) -> Result<Vec<RecordedEventV1>, EventJournalError> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        let after_global_position =
            i64::try_from(after_global_position).map_err(|_| EventJournalError::InvalidEvent {
                message: "after_global_position exceeds PostgreSQL BIGINT".to_string(),
            })?;
        let limit = i64::from(limit.min(1_000));
        let sql = format!(
            "SELECT {EVENT_SELECT}
             FROM recorded_events
             WHERE global_position > $1
             ORDER BY global_position ASC
             LIMIT $2"
        );
        let records = sqlx::query_as::<_, RecordedEventRecord>(&sql)
            .bind(after_global_position)
            .bind(limit)
            .fetch_all(&self.pool)
            .await
            .map_err(persistence_error)?;
        convert_records(records)
    }
}

fn validate_append(
    aggregate_id: &str,
    events: &[NewRecordedEventV1],
) -> Result<(), EventJournalError> {
    if aggregate_id.trim().is_empty() {
        return Err(EventJournalError::InvalidEvent {
            message: "aggregate_id must not be blank".to_string(),
        });
    }
    for event in events {
        if event.event_id.as_str().trim().is_empty() {
            return Err(EventJournalError::InvalidEvent {
                message: "event_id must not be blank".to_string(),
            });
        }
        if event.event_type.trim().is_empty() {
            return Err(EventJournalError::InvalidEvent {
                message: "event_type must not be blank".to_string(),
            });
        }
        if event.event_version == 0 {
            return Err(EventJournalError::InvalidEvent {
                message: "event_version must be positive".to_string(),
            });
        }
        if !event.payload.is_object() {
            return Err(EventJournalError::InvalidEvent {
                message: "payload must be a JSON object".to_string(),
            });
        }
    }
    Ok(())
}

fn convert_records(
    records: Vec<RecordedEventRecord>,
) -> Result<Vec<RecordedEventV1>, EventJournalError> {
    records.into_iter().map(TryInto::try_into).collect()
}

fn map_insert_error(
    error: sqlx::Error,
    event_id: openwork_protocol::domain::EventId,
) -> EventJournalError {
    if error
        .as_database_error()
        .and_then(|database_error| database_error.constraint())
        == Some("recorded_events_event_id_key")
    {
        EventJournalError::DuplicateEvent { event_id }
    } else {
        persistence_error(error)
    }
}

fn persistence_error(error: impl std::fmt::Display) -> EventJournalError {
    EventJournalError::Persistence {
        message: error.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use openwork_protocol::domain::EventId;

    use super::*;

    #[test]
    fn validation_rejects_live_delta_payload_primitives_and_blank_names() {
        let primitive_payload = NewRecordedEventV1::new(
            EventId::new("evt-1"),
            "text_delta",
            serde_json::json!("token"),
            1,
        );
        assert!(validate_append("turn-1", &[primitive_payload]).is_err());

        let blank_type =
            NewRecordedEventV1::new(EventId::new("evt-2"), " ", serde_json::json!({}), 1);
        assert!(validate_append("turn-1", &[blank_type]).is_err());
    }
}
