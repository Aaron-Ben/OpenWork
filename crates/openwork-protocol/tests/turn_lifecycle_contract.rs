use openwork_protocol::{
    approval::{ApprovalRequested, ApprovalResolution, ApprovalResolved},
    capability::Observation,
    domain::{ApprovalId, StepId, ToolRunId, TurnId},
    turn::{StepStarted, ToolRunFinished, ToolRunRequested, ToolRunStarted, TurnRecordedEvent},
};
use serde_json::json;

#[test]
fn approval_is_linked_to_the_step_and_tool_run_instead_of_a_provider_call_id() {
    let requested = ApprovalRequested {
        approval_id: ApprovalId::new("approval-1"),
        turn_id: TurnId::new("turn-1"),
        step_id: StepId::new("step-1"),
        tool_run_id: ToolRunId::new("tool-run-1"),
        tool_name: "bash".to_string(),
        input: json!({"command": "cargo test"}),
        reason: "process execution requires approval".to_string(),
    };
    let resolved = ApprovalResolved {
        approval_id: requested.approval_id.clone(),
        turn_id: requested.turn_id.clone(),
        step_id: requested.step_id.clone(),
        tool_run_id: requested.tool_run_id.clone(),
        resolution: ApprovalResolution::Allow,
    };

    assert_eq!(requested.step_id.as_str(), "step-1");
    assert_eq!(resolved.tool_run_id.as_str(), "tool-run-1");
}

#[test]
fn durable_turn_events_have_stable_event_type_names() {
    let step_id = StepId::new("step-1");
    let tool_run_id = ToolRunId::new("tool-run-1");
    let events = vec![
        TurnRecordedEvent::StepStarted(StepStarted {
            step_id: step_id.clone(),
            step_index: 1,
        }),
        TurnRecordedEvent::ToolRunRequested(ToolRunRequested {
            step_id: step_id.clone(),
            tool_run_id: tool_run_id.clone(),
            provider_tool_call_id: "call-1".to_string(),
            tool_name: "bash".to_string(),
            input: json!({"command": "cargo test"}),
        }),
        TurnRecordedEvent::ToolRunStarted(ToolRunStarted {
            step_id: step_id.clone(),
            tool_run_id: tool_run_id.clone(),
        }),
        TurnRecordedEvent::ToolRunFinished(ToolRunFinished {
            step_id,
            tool_run_id,
            observation: Observation::succeeded("ok"),
        }),
    ];

    assert_eq!(
        events
            .iter()
            .map(TurnRecordedEvent::event_type)
            .collect::<Vec<_>>(),
        vec![
            "step_started",
            "tool_run_requested",
            "tool_run_started",
            "tool_run_completed",
        ]
    );
}
