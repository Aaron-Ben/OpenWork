mod common;

use common::{connect_test_pool, test_config};
use openwork_persistence::{PostgresEventJournal, PostgresPersistence};
use openwork_protocol::{
    domain::EventId,
    journal::{
        AggregateType, EventJournal, EventJournalError, ExpectedVersion, NewRecordedEventV1,
    },
};
use uuid::Uuid;

async fn test_journal() -> Option<PostgresEventJournal> {
    let config = test_config(4)?;
    PostgresPersistence::migrate_database(config.clone())
        .await
        .ok()?;
    Some(PostgresEventJournal::new(
        connect_test_pool(&config).await.ok()?,
    ))
}

fn event(event_type: &str) -> NewRecordedEventV1 {
    NewRecordedEventV1::new(
        EventId::new(format!("evt-{}", Uuid::new_v4().simple())),
        event_type,
        serde_json::json!({}),
        1_752_166_800_000,
    )
}

#[tokio::test]
async fn append_assigns_contiguous_aggregate_versions_and_replays_in_order() {
    let Some(journal) = test_journal().await else {
        return;
    };
    let aggregate_id = format!("turn-{}", Uuid::new_v4().simple());

    let first = journal
        .append(
            AggregateType::Turn,
            &aggregate_id,
            ExpectedVersion::NoStream,
            vec![event("turn_started"), event("user_message_recorded")],
        )
        .await
        .unwrap();
    let second = journal
        .append(
            AggregateType::Turn,
            &aggregate_id,
            ExpectedVersion::Exact(2),
            vec![event("turn_completed")],
        )
        .await
        .unwrap();

    assert_eq!(first[0].aggregate_version, 1);
    assert_eq!(first[1].aggregate_version, 2);
    assert_eq!(second[0].aggregate_version, 3);
    assert!(second[0].global_position > first[1].global_position);
    assert_eq!(first[0].occurred_at_unix_ms, 1_752_166_800_000);

    let replayed = journal
        .load_aggregate(AggregateType::Turn, &aggregate_id, 0)
        .await
        .unwrap();
    assert_eq!(
        replayed
            .iter()
            .map(|event| event.event_type.as_str())
            .collect::<Vec<_>>(),
        ["turn_started", "user_message_recorded", "turn_completed"]
    );
}

#[tokio::test]
async fn concurrent_writers_cannot_both_claim_the_same_expected_version() {
    let Some(journal) = test_journal().await else {
        return;
    };
    let aggregate_id = format!("turn-{}", Uuid::new_v4().simple());
    let left = journal.clone();
    let right = journal.clone();
    let left_id = aggregate_id.clone();
    let right_id = aggregate_id.clone();

    let (left_result, right_result) = tokio::join!(
        left.append(
            AggregateType::Turn,
            &left_id,
            ExpectedVersion::NoStream,
            vec![event("turn_started")],
        ),
        right.append(
            AggregateType::Turn,
            &right_id,
            ExpectedVersion::NoStream,
            vec![event("turn_started")],
        )
    );

    let successes = usize::from(left_result.is_ok()) + usize::from(right_result.is_ok());
    assert_eq!(successes, 1);
    let conflict = if let Err(error) = left_result {
        error
    } else {
        right_result.unwrap_err()
    };
    assert!(matches!(
        conflict,
        EventJournalError::VersionConflict {
            expected: 0,
            actual: 1,
            ..
        }
    ));
}

#[tokio::test]
async fn duplicate_event_id_is_rejected_without_creating_a_second_fact() {
    let Some(journal) = test_journal().await else {
        return;
    };
    let first_aggregate = format!("turn-{}", Uuid::new_v4().simple());
    let second_aggregate = format!("turn-{}", Uuid::new_v4().simple());
    let duplicate = event("turn_started");

    journal
        .append(
            AggregateType::Turn,
            &first_aggregate,
            ExpectedVersion::NoStream,
            vec![duplicate.clone()],
        )
        .await
        .unwrap();
    let error = journal
        .append(
            AggregateType::Turn,
            &second_aggregate,
            ExpectedVersion::NoStream,
            vec![duplicate.clone()],
        )
        .await
        .unwrap_err();

    assert!(matches!(
        error,
        EventJournalError::DuplicateEvent { event_id }
            if event_id == duplicate.event_id
    ));
    assert!(
        journal
            .load_aggregate(AggregateType::Turn, &second_aggregate, 0)
            .await
            .unwrap()
            .is_empty()
    );
}
