use std::time::Duration;

use openwork_collab::domain::{MessageFingerprint, next_sequence, unread_count};

#[test]
fn room_sequence_advances_once_and_rejects_overflow() {
    assert_eq!(next_sequence(0).unwrap(), 1);
    assert_eq!(next_sequence(41).unwrap(), 42);
    assert!(next_sequence(i64::MAX).is_err());
}

#[test]
fn duplicate_is_exact_and_limited_to_the_window() {
    let original = MessageFingerprint {
        author_id: "alice",
        body: "Hello",
        age: Duration::from_millis(2_999),
    };

    assert!(original.is_duplicate_of("alice", "Hello", Duration::from_secs(3)));
    assert!(!original.is_duplicate_of("alice", "hello", Duration::from_secs(3)));
    assert!(!original.is_duplicate_of("bob", "Hello", Duration::from_secs(3)));
    assert!(
        !MessageFingerprint {
            age: Duration::from_millis(3_001),
            ..original
        }
        .is_duplicate_of("alice", "Hello", Duration::from_secs(3))
    );
}

#[test]
fn unread_count_never_moves_the_read_cursor() {
    assert_eq!(unread_count(4, 9), 5);
    assert_eq!(unread_count(9, 9), 0);
    assert_eq!(unread_count(12, 9), 0);
}
