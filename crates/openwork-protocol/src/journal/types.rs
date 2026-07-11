use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::domain::EventId;

/// The domain aggregate whose ordered fact stream owns an event.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AggregateType {
    Thread,
    Turn,
}

impl AggregateType {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Thread => "thread",
            Self::Turn => "turn",
        }
    }
}

impl std::fmt::Display for AggregateType {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// Compare-and-swap precondition for an aggregate append.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExpectedVersion {
    /// The first event may only be appended when the aggregate has no stream.
    NoStream,
    /// The append may only proceed when this is the current aggregate version.
    Exact(u64),
}

impl ExpectedVersion {
    pub const fn expected_current_version(self) -> u64 {
        match self {
            Self::NoStream => 0,
            Self::Exact(version) => version,
        }
    }
}

/// A fact before database-assigned positions and record time are known.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NewRecordedEventV1 {
    pub event_id: EventId,
    pub event_type: String,
    pub event_version: u32,
    pub payload: Value,
    pub occurred_at_unix_ms: i64,
}

impl NewRecordedEventV1 {
    pub fn new(
        event_id: EventId,
        event_type: impl Into<String>,
        payload: Value,
        occurred_at_unix_ms: i64,
    ) -> Self {
        Self {
            event_id,
            event_type: event_type.into(),
            event_version: 1,
            payload,
            occurred_at_unix_ms,
        }
    }
}

/// Version-one durable event envelope returned by the Journal.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordedEventV1 {
    pub global_position: u64,
    pub event_id: EventId,
    pub aggregate_type: AggregateType,
    pub aggregate_id: String,
    pub aggregate_version: u64,
    pub event_type: String,
    pub event_version: u32,
    pub payload: Value,
    pub occurred_at_unix_ms: i64,
    pub recorded_at_unix_ms: i64,
}
