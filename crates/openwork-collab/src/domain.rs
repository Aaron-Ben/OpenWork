use std::time::Duration;

use thiserror::Error;

#[derive(Debug, Clone, Copy)]
pub struct MessageFingerprint<'a> {
    pub author_id: &'a str,
    pub body: &'a str,
    pub age: Duration,
}

impl MessageFingerprint<'_> {
    pub fn is_duplicate_of(&self, author_id: &str, body: &str, window: Duration) -> bool {
        self.author_id == author_id && self.body == body && self.age <= window
    }
}

pub fn next_sequence(current: i64) -> Result<i64, DomainError> {
    current.checked_add(1).ok_or(DomainError::SequenceOverflow)
}

pub fn unread_count(last_read_sequence: i64, highest_sequence: i64) -> u64 {
    highest_sequence
        .saturating_sub(last_read_sequence)
        .try_into()
        .unwrap_or(0)
}

#[derive(Debug, Clone, Copy, Error, PartialEq, Eq)]
pub enum DomainError {
    #[error("room sequence is exhausted")]
    SequenceOverflow,
}
