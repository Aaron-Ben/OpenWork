//! Durable recorded-event contracts.
//!
//! This module contains domain-facing types and ports only. PostgreSQL rows,
//! migrations, projectors, and live UI deltas belong to adapter crates.

mod port;
mod types;

pub use port::{EventJournal, EventJournalError};
pub use types::{AggregateType, ExpectedVersion, NewRecordedEventV1, RecordedEventV1};
