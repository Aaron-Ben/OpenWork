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

#[test]
fn an_ack_with_assistant_text_is_still_silent() {
    let sink = ObservationSink::discarding();
    sink.set_active_run("alice", Some("run_ack_with_text"));
    sink.mark_ack("alice", "quiet_room");
    sink.observe_assistant_text(
        "run_ack_with_text",
        &event(json!({
            "type": "message.updated",
            "properties": {"info": {"id": "msg_assistant", "role": "assistant"}}
        })),
    );
    sink.observe_assistant_text(
        "run_ack_with_text",
        &event(json!({
            "type": "message.part.updated",
            "properties": {"part": {
                "id": "prt_ack", "messageID": "msg_assistant", "type": "text",
                "text": "我已经看过这个房间，这件事明确交给 Bob 处理，我这轮不再重复回复。"
            }}
        })),
    );

    assert_eq!(
        sink.take_run_evidence("run_ack_with_text").outcome(),
        RunOutcome::Silent
    );
}

#[test]
fn run_outcome_classification_covers_every_tool_and_text_combination() {
    struct Case {
        name: &'static str,
        acted: bool,
        acked: bool,
        has_long_text: bool,
        expected: RunOutcome,
    }

    let cases = [
        Case {
            name: "a publishing action is acted",
            acted: true,
            acked: false,
            has_long_text: false,
            expected: RunOutcome::Acted,
        },
        Case {
            name: "ack without text is silent",
            acted: false,
            acked: true,
            has_long_text: false,
            expected: RunOutcome::Silent,
        },
        Case {
            name: "ack with explanatory text is silent",
            acted: false,
            acked: true,
            has_long_text: true,
            expected: RunOutcome::Silent,
        },
        Case {
            name: "a publishing action wins over ack",
            acted: true,
            acked: true,
            has_long_text: true,
            expected: RunOutcome::Acted,
        },
        Case {
            name: "long text without a settling tool is unpublished",
            acted: false,
            acked: false,
            has_long_text: true,
            expected: RunOutcome::Unpublished,
        },
        Case {
            name: "no settling tool and no text is silent",
            acted: false,
            acked: false,
            has_long_text: false,
            expected: RunOutcome::Silent,
        },
    ];

    for (index, case) in cases.iter().enumerate() {
        let sink = ObservationSink::discarding();
        let run_id = format!("run_outcome_{index}");
        let message_id = format!("msg_outcome_{index}");
        sink.set_active_run("alice", Some(&run_id));
        if case.acted {
            sink.mark_action("alice", "published_room");
        }
        if case.acked {
            sink.mark_ack("alice", "acked_room");
        }
        if case.has_long_text {
            sink.observe_assistant_text(
                &run_id,
                &event(json!({
                    "type": "message.updated",
                    "properties": {"info": {"id": message_id, "role": "assistant"}}
                })),
            );
            sink.observe_assistant_text(
                &run_id,
                &event(json!({
                    "type": "message.part.updated",
                    "properties": {"part": {
                        "id": format!("prt_outcome_{index}"),
                        "messageID": message_id,
                        "type": "text",
                        "text": "这是一段真实长度的 assistant 正文，用来确认分类器不会遗漏工具与正文的组合。"
                    }}
                })),
            );
        }

        assert_eq!(
            sink.take_run_evidence(&run_id).outcome(),
            case.expected,
            "{}",
            case.name
        );
    }
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
