use openwork_collab::{
    observation::{EngineObservation, normalize_engine_observation},
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
