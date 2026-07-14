use openwork_protocol::{
    domain::EventId,
    journal::{AggregateType, EventJournalError, RecordedEventV1},
};
use sqlx::FromRow;

#[derive(Debug, Clone, FromRow)]
pub(super) struct RecordedEventRecord {
    pub global_position: i64,
    pub event_id: String,
    pub aggregate_type: String,
    pub aggregate_id: String,
    pub aggregate_version: i64,
    pub event_type: String,
    pub event_version: i32,
    pub payload_json: serde_json::Value,
    pub occurred_at_unix_ms: i64,
    pub recorded_at_unix_ms: i64,
}

impl TryFrom<RecordedEventRecord> for RecordedEventV1 {
    type Error = EventJournalError;

    fn try_from(record: RecordedEventRecord) -> Result<Self, Self::Error> {
        let aggregate_type = match record.aggregate_type.as_str() {
            "session" => AggregateType::Session,
            "turn" => AggregateType::Turn,
            value => {
                return Err(EventJournalError::Persistence {
                    message: format!("unknown aggregate_type read from journal: {value}"),
                });
            }
        };

        Ok(Self {
            global_position: unsigned(record.global_position, "global_position")?,
            event_id: EventId::new(record.event_id),
            aggregate_type,
            aggregate_id: record.aggregate_id,
            aggregate_version: unsigned(record.aggregate_version, "aggregate_version")?,
            event_type: record.event_type,
            event_version: u32::try_from(record.event_version).map_err(|_| {
                EventJournalError::Persistence {
                    message: format!(
                        "event_version must be non-negative, got {}",
                        record.event_version
                    ),
                }
            })?,
            payload: record.payload_json,
            occurred_at_unix_ms: record.occurred_at_unix_ms,
            recorded_at_unix_ms: record.recorded_at_unix_ms,
        })
    }
}

fn unsigned(value: i64, field: &str) -> Result<u64, EventJournalError> {
    u64::try_from(value).map_err(|_| EventJournalError::Persistence {
        message: format!("{field} must be non-negative, got {value}"),
    })
}
