use openwork_protocol::{
    domain::EventId,
    journal::{AggregateType, ExpectedVersion, NewRecordedEventV1, RecordedEventV1},
};

#[test]
fn recorded_event_envelope_roundtrips_without_transport_or_database_types() {
    let event = RecordedEventV1 {
        global_position: 42,
        event_id: EventId::new("evt-1"),
        aggregate_type: AggregateType::Turn,
        aggregate_id: "turn-1".to_string(),
        aggregate_version: 3,
        event_type: "approval_resolved".to_string(),
        event_version: 1,
        payload: serde_json::json!({
            "approvalId": "approval-1",
            "resolution": "allow"
        }),
        occurred_at_unix_ms: 1_752_166_800_000,
        recorded_at_unix_ms: 1_752_166_800_015,
    };

    let json = serde_json::to_string(&event).unwrap();
    let decoded: RecordedEventV1 = serde_json::from_str(&json).unwrap();

    assert_eq!(decoded, event);
    assert!(json.contains("\"aggregateType\":\"turn\""));
    assert!(json.contains("\"eventType\":\"approval_resolved\""));
}

#[test]
fn new_event_carries_fact_data_but_not_database_assigned_positions() {
    let event = NewRecordedEventV1::new(
        EventId::new("evt-2"),
        "turn_started",
        serde_json::json!({"sessionId": "session-1"}),
        1_752_166_800_000,
    );

    assert_eq!(event.event_version, 1);
    assert_eq!(event.event_type, "turn_started");
    assert_eq!(event.payload["sessionId"], "session-1");
}

#[test]
fn session_is_the_durable_conversation_aggregate_name() {
    assert_eq!(AggregateType::Session.as_str(), "session");
    assert_eq!(
        serde_json::to_string(&AggregateType::Session).unwrap(),
        "\"session\""
    );
}

#[test]
fn expected_version_makes_first_write_and_compare_and_swap_explicit() {
    assert_eq!(ExpectedVersion::NoStream.expected_current_version(), 0);
    assert_eq!(ExpectedVersion::Exact(7).expected_current_version(), 7);
}
