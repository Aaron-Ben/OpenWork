use openwork_collab::{
    model::RunOutcome,
    observation::{EngineObservation, ObservationSink, normalize_engine_observation},
    opencode::GlobalEvent,
};
use serde_json::json;

fn event(payload: serde_json::Value) -> GlobalEvent {
    GlobalEvent {
        directory: None,
        project: None,
        payload,
    }
}

#[test]
fn run_outcomes_are_derived_from_action_calls_and_assistant_text() {
    let acted = ObservationSink::discarding();
    acted.set_active_run("alice", Some("run_acted"));
    acted.mark_action("alice", "general");
    let evidence = acted.take_run_evidence("run_acted");
    assert_eq!(evidence.outcome(), RunOutcome::Acted);
    assert_eq!(
        evidence.settled_rooms().collect::<Vec<_>>(),
        vec!["general"]
    );

    let silent = ObservationSink::discarding();
    silent.set_active_run("alice", Some("run_silent"));
    silent.observe_assistant_text(
        "run_silent",
        &event(json!({
            "type": "message.updated",
            "properties": {"info": {"id": "msg_assistant", "role": "assistant"}}
        })),
    );
    silent.observe_assistant_text(
        "run_silent",
        &event(json!({
            "type": "message.part.updated",
            "properties": {"part": {
                "id": "prt_1", "messageID": "msg_assistant", "type": "text", "text": "..."
            }}
        })),
    );
    assert_eq!(
        silent.take_run_evidence("run_silent").outcome(),
        RunOutcome::Silent
    );

    let unpublished = ObservationSink::discarding();
    unpublished.set_active_run("alice", Some("run_unpublished"));
    unpublished.observe_assistant_text(
        "run_unpublished",
        &event(json!({
            "type": "message.updated",
            "properties": {"info": {"id": "msg_assistant", "role": "assistant"}}
        })),
    );
    let update = |text: &str| {
        event(json!({
            "type": "message.part.updated",
            "properties": {"part": {
                "id": "prt_1", "messageID": "msg_assistant", "type": "text", "text": text
            }}
        }))
    };
    unpublished.observe_assistant_text("run_unpublished", &update("这是一段"));
    unpublished.observe_assistant_text(
        "run_unpublished",
        &update("这是一段没有通过 reply 工具发布的完整正文。"),
    );
    assert_eq!(
        unpublished.take_run_evidence("run_unpublished").outcome(),
        RunOutcome::Unpublished
    );

    let user_text = ObservationSink::discarding();
    user_text.set_active_run("alice", Some("run_user_text"));
    user_text.observe_assistant_text(
        "run_user_text",
        &event(json!({
            "type": "message.updated",
            "properties": {"info": {"id": "msg_user", "role": "user"}}
        })),
    );
    user_text.observe_assistant_text(
        "run_user_text",
        &event(json!({
            "type": "message.part.updated",
            "properties": {"part": {
                "id": "prt_user", "messageID": "msg_user", "type": "text",
                "text": "这是很长的用户提示词，不应成为 Agent 的未发布正文。"
            }}
        })),
    );
    assert_eq!(
        user_text.take_run_evidence("run_user_text").outcome(),
        RunOutcome::Silent
    );
}

#[test]
fn engine_tool_command_and_usage_events_are_normalized_without_losing_payloads() {
    let command = normalize_engine_observation(&event(json!({
        "type": "message.part.updated",
        "properties": {
            "part": {
                "id": "prt_1",
                "sessionID": "ses_1",
                "type": "tool",
                "tool": "bash",
                "state": {"status": "completed", "input": {"command": "cargo test"}}
            }
        }
    })))
    .unwrap();
    assert_eq!(command.kind, "command.execution");
    assert_eq!(command.payload["tool"], "bash");
    assert_eq!(command.payload["status"], "completed");

    let tool = normalize_engine_observation(&event(json!({
        "type": "message.part.updated",
        "properties": {
            "part": {
                "id": "prt_2",
                "sessionID": "ses_1",
                "type": "tool",
                "tool": "read",
                "state": {"status": "running", "input": {"filePath": "README.md"}}
            }
        }
    })))
    .unwrap();
    assert_eq!(tool.kind, "tool.execution");
    assert_eq!(tool.payload["tool"], "read");

    let usage = normalize_engine_observation(&event(json!({
        "type": "message.updated",
        "properties": {
            "info": {
                "id": "msg_1",
                "sessionID": "ses_1",
                "role": "assistant",
                "tokens": {
                    "input": 120,
                    "output": 40,
                    "reasoning": 10,
                    "cache": {"read": 30, "write": 2}
                }
            }
        }
    })))
    .unwrap();
    assert_eq!(
        usage,
        EngineObservation {
            kind: "usage.reported",
            payload: json!({
                "messageId": "msg_1",
                "inputTokens": 120,
                "cachedInputTokens": 30,
                "outputTokens": 40,
                "reasoningTokens": 10,
                "cacheWriteTokens": 2
            }),
            usage: Some(openwork_collab::observation::TokenUsage {
                input_tokens: 120,
                cached_input_tokens: 30,
                output_tokens: 40,
            }),
        }
    );
}

#[test]
fn irrelevant_engine_events_do_not_enter_the_durable_observation_stream() {
    assert_eq!(
        normalize_engine_observation(&event(json!({
            "type": "message.part.delta",
            "properties": {"sessionID": "ses_1", "delta": "private reasoning"}
        }))),
        None
    );
    assert_eq!(
        normalize_engine_observation(&event(json!({
            "type": "message.updated",
            "properties": {"info": {
                "id": "msg_zero", "sessionID": "ses_1", "role": "assistant",
                "tokens": {"input": 0, "output": 0, "reasoning": 0, "cache": {"read": 0, "write": 0}}
            }}
        }))),
        None,
        "the initial all-zero usage frame is not durable evidence",
    );
}

/// Standing down settles a room's delivery without counting as a response:
/// the room stops being redelivered, but the turn still reports `silent`.
#[test]
fn an_ack_settles_its_room_without_being_recorded_as_a_response() {
    let sink = ObservationSink::discarding();
    sink.set_active_run("alice", Some("run_ack"));
    sink.mark_ack("alice", "quiet_room");
    let evidence = sink.take_run_evidence("run_ack");
    assert_eq!(evidence.outcome(), RunOutcome::Silent);
    assert!(evidence.acted_rooms().is_empty());
    assert_eq!(
        evidence.settled_rooms().collect::<Vec<_>>(),
        vec!["quiet_room"]
    );
}

/// Rooms shown to the engine but neither answered nor acked must NOT settle —
/// that is the whole reason settlement is per room rather than per run.
#[test]
fn a_room_that_was_only_shown_never_settles() {
    let sink = ObservationSink::discarding();
    sink.set_active_run("alice", Some("run_partial"));
    sink.mark_action("alice", "answered");
    let evidence = sink.take_run_evidence("run_partial");
    let settled = evidence.settled_rooms().collect::<Vec<_>>();
    assert_eq!(settled, vec!["answered"]);
    assert!(!settled.contains(&"only_shown"));
}
