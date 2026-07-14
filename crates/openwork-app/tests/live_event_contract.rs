use openwork_app::{TurnLiveEvent, TurnLiveEventKind};
use serde_json::json;

#[test]
fn text_delta_serializes_as_a_tagged_event_without_unrelated_optional_fields() {
    let event = TurnLiveEvent::new(
        "req-1",
        "sess-1",
        TurnLiveEventKind::TextDelta {
            delta: "hello".to_string(),
        },
    );

    assert_eq!(
        serde_json::to_value(event).unwrap(),
        json!({
            "requestId": "req-1",
            "sessionId": "sess-1",
            "event": "text_delta",
            "delta": "hello"
        })
    );
}

#[test]
fn approval_and_tool_events_require_their_variant_specific_payload() {
    let approval = TurnLiveEvent::new(
        "req-2",
        "sess-2",
        TurnLiveEventKind::ApprovalRequest {
            approval_id: "approval-1".to_string(),
            tool_run_id: "tool-run-1".to_string(),
            tool_name: "bash".to_string(),
            input: json!({ "command": "pwd" }),
            reason: "process execution requires approval".to_string(),
        },
    );
    let tool_result = TurnLiveEvent::new(
        "req-2",
        "sess-2",
        TurnLiveEventKind::ToolResult {
            tool_call_id: "call-1".to_string(),
            tool_run_id: "tool-run-1".to_string(),
            tool_name: "bash".to_string(),
            output: "/workspace".to_string(),
            is_error: false,
        },
    );

    assert_eq!(
        serde_json::to_value(approval).unwrap(),
        json!({
            "requestId": "req-2",
            "sessionId": "sess-2",
            "event": "approval_request",
            "approvalId": "approval-1",
            "toolRunId": "tool-run-1",
            "toolName": "bash",
            "input": { "command": "pwd" },
            "reason": "process execution requires approval"
        })
    );
    assert_eq!(
        serde_json::to_value(tool_result).unwrap(),
        json!({
            "requestId": "req-2",
            "sessionId": "sess-2",
            "event": "tool_result",
            "toolCallId": "call-1",
            "toolRunId": "tool-run-1",
            "toolName": "bash",
            "output": "/workspace",
            "isError": false
        })
    );
}
