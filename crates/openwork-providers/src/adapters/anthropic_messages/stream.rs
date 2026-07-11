//! Anthropic Messages stream event codec boundary.

use openwork_protocol::{
    model::{ModelEvent, ProviderOpaqueBlock, ToolCallBlock, ToolCallState},
    provider::ProviderDriver,
};
use serde_json::{Value, json};

#[derive(Debug)]
struct PendingTool {
    index: u32,
    id: String,
    name: String,
    input: String,
}

#[derive(Debug, Default)]
pub(crate) struct AnthropicToolStream {
    tools: Vec<PendingTool>,
}

impl AnthropicToolStream {
    pub(crate) fn observe(&mut self, event: &Value) -> Vec<ModelEvent> {
        let index = event.get("index").and_then(Value::as_u64).unwrap_or(0) as u32;
        match event.get("type").and_then(Value::as_str) {
            Some("content_block_start")
                if event.pointer("/content_block/type").and_then(Value::as_str)
                    == Some("tool_use") =>
            {
                let Some(id) = event.pointer("/content_block/id").and_then(Value::as_str) else {
                    return Vec::new();
                };
                let Some(name) = event.pointer("/content_block/name").and_then(Value::as_str)
                else {
                    return Vec::new();
                };
                self.tools.push(PendingTool {
                    index,
                    id: id.to_string(),
                    name: name.to_string(),
                    input: String::new(),
                });
                vec![ModelEvent::ToolCallStart {
                    index,
                    id: id.to_string(),
                    name: name.to_string(),
                }]
            }
            Some("content_block_delta")
                if event.pointer("/delta/type").and_then(Value::as_str)
                    == Some("input_json_delta") =>
            {
                let Some(delta) = event.pointer("/delta/partial_json").and_then(Value::as_str)
                else {
                    return Vec::new();
                };
                let Some(tool) = self.tools.iter_mut().find(|tool| tool.index == index) else {
                    return Vec::new();
                };
                tool.input.push_str(delta);
                vec![ModelEvent::ToolCallDelta {
                    index,
                    id: tool.id.clone(),
                    partial_input: delta.to_string(),
                }]
            }
            _ => Vec::new(),
        }
    }

    pub(crate) fn finish(self) -> Result<(Vec<ToolCallBlock>, Vec<ModelEvent>), String> {
        let mut calls = Vec::with_capacity(self.tools.len());
        let mut ends = Vec::with_capacity(self.tools.len());
        for tool in self.tools {
            let input = if tool.input.is_empty() {
                "{}".to_string()
            } else {
                tool.input
            };
            serde_json::from_str::<Value>(&input).map_err(|error| {
                format!(
                    "Anthropic tool call '{}' produced invalid JSON input: {error}",
                    tool.name
                )
            })?;
            ends.push(ModelEvent::ToolCallEnd {
                index: tool.index,
                id: tool.id.clone(),
            });
            calls.push(ToolCallBlock {
                id: tool.id,
                name: tool.name,
                input,
                state: ToolCallState::Submitted,
            });
        }
        Ok((calls, ends))
    }
}

#[derive(Debug)]
struct PendingThinking {
    index: u32,
    thinking: String,
    signature: Option<String>,
}

#[derive(Debug, Default)]
pub(crate) struct AnthropicThinkingStream {
    thinking: Vec<PendingThinking>,
    redacted: Vec<(u32, Value)>,
}

impl AnthropicThinkingStream {
    pub(crate) fn observe(&mut self, event: &Value) {
        let index = event.get("index").and_then(Value::as_u64).unwrap_or(0) as u32;
        match event.get("type").and_then(Value::as_str) {
            Some("content_block_start") => {
                match event.pointer("/content_block/type").and_then(Value::as_str) {
                    Some("thinking") => self.thinking.push(PendingThinking {
                        index,
                        thinking: event
                            .pointer("/content_block/thinking")
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_string(),
                        signature: event
                            .pointer("/content_block/signature")
                            .and_then(Value::as_str)
                            .map(ToOwned::to_owned),
                    }),
                    Some("redacted_thinking") => {
                        if let Some(block) = event.get("content_block") {
                            self.redacted.push((index, block.clone()));
                        }
                    }
                    _ => {}
                }
            }
            Some("content_block_delta") => {
                let Some(block) = self.thinking.iter_mut().find(|block| block.index == index)
                else {
                    return;
                };
                match event.pointer("/delta/type").and_then(Value::as_str) {
                    Some("thinking_delta") => {
                        if let Some(delta) =
                            event.pointer("/delta/thinking").and_then(Value::as_str)
                        {
                            block.thinking.push_str(delta);
                        }
                    }
                    Some("signature_delta") => {
                        block.signature = event
                            .pointer("/delta/signature")
                            .and_then(Value::as_str)
                            .map(ToOwned::to_owned);
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }

    pub(crate) fn finish(self) -> Vec<ProviderOpaqueBlock> {
        let mut blocks: Vec<(u32, ProviderOpaqueBlock)> = self
            .thinking
            .into_iter()
            .map(|block| {
                let mut payload = json!({
                    "type": "thinking",
                    "thinking": block.thinking,
                });
                if let Some(signature) = block.signature {
                    payload["signature"] = json!(signature);
                }
                (
                    block.index,
                    ProviderOpaqueBlock {
                        driver: ProviderDriver::AnthropicMessages,
                        kind: "thinking".to_string(),
                        payload,
                    },
                )
            })
            .chain(self.redacted.into_iter().map(|(index, payload)| {
                (
                    index,
                    ProviderOpaqueBlock {
                        driver: ProviderDriver::AnthropicMessages,
                        kind: "redacted_thinking".to_string(),
                        payload,
                    },
                )
            }))
            .collect();
        blocks.sort_by_key(|(index, _)| *index);
        blocks.into_iter().map(|(_, block)| block).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use openwork_protocol::model::ModelEvent;
    use serde_json::json;

    #[test]
    fn accumulates_tool_use_input_json_deltas() {
        let mut stream = AnthropicToolStream::default();
        let started = stream.observe(&json!({
            "type": "content_block_start",
            "index": 1,
            "content_block": {
                "type": "tool_use",
                "id": "toolu_1",
                "name": "read",
                "input": {}
            }
        }));
        let delta = stream.observe(&json!({
            "type": "content_block_delta",
            "index": 1,
            "delta": {
                "type": "input_json_delta",
                "partial_json": "{\"path\":\"Cargo.toml\"}"
            }
        }));

        assert!(matches!(
            started[0],
            ModelEvent::ToolCallStart { index: 1, .. }
        ));
        assert!(matches!(
            delta[0],
            ModelEvent::ToolCallDelta { index: 1, .. }
        ));
        let (calls, ends) = stream.finish().unwrap();
        assert_eq!(calls[0].id, "toolu_1");
        assert_eq!(calls[0].input, r#"{"path":"Cargo.toml"}"#);
        assert!(matches!(ends[0], ModelEvent::ToolCallEnd { index: 1, .. }));
    }

    #[test]
    fn preserves_thinking_signature_and_redacted_blocks() {
        let mut stream = AnthropicThinkingStream::default();
        stream.observe(&json!({
            "type": "content_block_start",
            "index": 0,
            "content_block": { "type": "thinking", "thinking": "" }
        }));
        stream.observe(&json!({
            "type": "content_block_delta",
            "index": 0,
            "delta": { "type": "thinking_delta", "thinking": "summary" }
        }));
        stream.observe(&json!({
            "type": "content_block_delta",
            "index": 0,
            "delta": { "type": "signature_delta", "signature": "signed-state" }
        }));
        stream.observe(&json!({
            "type": "content_block_start",
            "index": 1,
            "content_block": { "type": "redacted_thinking", "data": "encrypted" }
        }));

        let blocks = stream.finish();
        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[0].kind, "thinking");
        assert_eq!(blocks[0].payload["signature"], "signed-state");
        assert_eq!(blocks[1].kind, "redacted_thinking");
        assert_eq!(blocks[1].payload["data"], "encrypted");
    }
}
