use openwork_persistence::{ToolRunLifecycleStatus, TurnLifecycleStatus, replay_turn_lifecycle};
use openwork_protocol::{
    domain::EventId,
    journal::{AggregateType, RecordedEventV1},
};
use serde_json::{Value, json};

fn event(version: u64, event_type: &str, payload: Value) -> RecordedEventV1 {
    RecordedEventV1 {
        global_position: version,
        event_id: EventId::new(format!("event-{version}")),
        aggregate_type: AggregateType::Turn,
        aggregate_id: "turn-1".to_string(),
        aggregate_version: version,
        event_type: event_type.to_string(),
        event_version: 1,
        payload,
        occurred_at_unix_ms: version as i64 * 1_000,
        recorded_at_unix_ms: version as i64 * 1_000,
    }
}

fn turn_started() -> RecordedEventV1 {
    event(
        1,
        "turn_started",
        json!({
            "sessionId": "session-1",
            "providerId": "provider-1",
            "model": "model-1"
        }),
    )
}

#[test]
fn replay_restores_a_pending_approval_with_its_tool_context() {
    let events = vec![
        turn_started(),
        event(
            2,
            "step_started",
            json!({"stepId": "step-1", "stepIndex": 1}),
        ),
        event(
            3,
            "tool_run_requested",
            json!({
                "stepId": "step-1",
                "toolRunId": "tool-run-1",
                "providerToolCallId": "call-1",
                "toolName": "bash",
                "input": {"command": "cargo test"}
            }),
        ),
        event(
            4,
            "approval_requested",
            json!({
                "approvalId": "approval-1",
                "turnId": "turn-1",
                "stepId": "step-1",
                "toolRunId": "tool-run-1",
                "toolName": "bash",
                "input": {"command": "cargo test"},
                "reason": "process execution requires approval"
            }),
        ),
    ];

    let snapshot = replay_turn_lifecycle(&events)
        .expect("valid event stream")
        .expect("turn snapshot");

    assert_eq!(snapshot.status, TurnLifecycleStatus::WaitingApproval);
    let pending = snapshot.pending_approval.expect("pending approval");
    assert_eq!(pending.approval_id, "approval-1");
    assert_eq!(pending.tool_run_id, "tool-run-1");
    assert_eq!(pending.provider_tool_call_id, "call-1");
    assert_eq!(pending.step_index, 1);
}

#[test]
fn replay_never_treats_an_unfinished_started_tool_as_safe_to_retry() {
    let events = vec![
        turn_started(),
        event(
            2,
            "step_started",
            json!({"stepId": "step-1", "stepIndex": 1}),
        ),
        event(
            3,
            "tool_run_requested",
            json!({
                "stepId": "step-1",
                "toolRunId": "tool-run-1",
                "providerToolCallId": "call-1",
                "toolName": "bash",
                "input": {"command": "touch side-effect"}
            }),
        ),
        event(
            4,
            "tool_run_started",
            json!({"stepId": "step-1", "toolRunId": "tool-run-1"}),
        ),
    ];

    let snapshot = replay_turn_lifecycle(&events)
        .expect("valid event stream")
        .expect("turn snapshot");

    assert_eq!(snapshot.status, TurnLifecycleStatus::OutcomeUnknown);
    assert_eq!(
        snapshot.steps[0].tool_runs[0].status,
        ToolRunLifecycleStatus::OutcomeUnknown
    );
    assert!(snapshot.pending_approval.is_none());
}

#[test]
fn a_turn_terminal_fact_cannot_hide_an_unconfirmed_tool_outcome() {
    let events = vec![
        turn_started(),
        event(
            2,
            "step_started",
            json!({"stepId": "step-1", "stepIndex": 1}),
        ),
        event(
            3,
            "tool_run_requested",
            json!({
                "stepId": "step-1",
                "toolRunId": "tool-run-1",
                "providerToolCallId": "call-1",
                "toolName": "bash",
                "input": {"command": "touch side-effect"}
            }),
        ),
        event(
            4,
            "tool_run_started",
            json!({"stepId": "step-1", "toolRunId": "tool-run-1"}),
        ),
        event(5, "turn_failed", json!({"message": "journal unavailable"})),
    ];

    let snapshot = replay_turn_lifecycle(&events)
        .expect("valid event stream")
        .expect("turn snapshot");

    assert_eq!(snapshot.status, TurnLifecycleStatus::OutcomeUnknown);
    assert_eq!(
        snapshot.steps[0].tool_runs[0].status,
        ToolRunLifecycleStatus::OutcomeUnknown
    );
}
