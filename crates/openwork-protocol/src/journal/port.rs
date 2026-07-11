use async_trait::async_trait;

use crate::domain::EventId;

use super::{AggregateType, ExpectedVersion, NewRecordedEventV1, RecordedEventV1};

#[derive(Debug, thiserror::Error)]
pub enum EventJournalError {
    #[error(
        "aggregate version conflict for {aggregate_type}/{aggregate_id}: expected {expected}, actual {actual}"
    )]
    VersionConflict {
        aggregate_type: AggregateType,
        aggregate_id: String,
        expected: u64,
        actual: u64,
    },
    #[error("duplicate recorded event: {event_id}")]
    DuplicateEvent { event_id: EventId },
    #[error("invalid recorded event: {message}")]
    InvalidEvent { message: String },
    #[error("event journal persistence error: {message}")]
    Persistence { message: String },
}

/// Append-only fact store used by Core and application services.
#[async_trait]
pub trait EventJournal: Send + Sync {
    async fn append(
        &self,
        aggregate_type: AggregateType,
        aggregate_id: &str,
        expected_version: ExpectedVersion,
        events: Vec<NewRecordedEventV1>,
    ) -> Result<Vec<RecordedEventV1>, EventJournalError>;

    async fn load_aggregate(
        &self,
        aggregate_type: AggregateType,
        aggregate_id: &str,
        after_version: u64,
    ) -> Result<Vec<RecordedEventV1>, EventJournalError>;

    async fn read_all(
        &self,
        after_global_position: u64,
        limit: u32,
    ) -> Result<Vec<RecordedEventV1>, EventJournalError>;
}
