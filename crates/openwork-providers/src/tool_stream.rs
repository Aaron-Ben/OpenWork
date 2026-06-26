use openwork_protocol::ai::{GenerateStreamEvent, ToolCallBlock, ToolCallState};
use serde_json::Value;

#[derive(Debug, Clone)]
struct PendingTool {
    id: Option<String>,
    name: Option<String>,
    input: String,
    started: bool,
    stable_id: String,
}

#[derive(Debug, Default)]
pub(crate) struct ToolStream {
    by_index: Vec<(u32, PendingTool)>,
}

impl ToolStream {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn append_openai_chat_delta(&mut self, tc: &Value) -> Vec<GenerateStreamEvent> {
        let mut events = Vec::new();
        let index = tc
            .get("index")
            .and_then(Value::as_u64)
            .map(|i| i as u32)
            .unwrap_or(0);
        let new_id = tc.get("id").and_then(Value::as_str).map(String::from);
        let new_name = tc
            .get("function")
            .and_then(|f| f.get("name"))
            .and_then(Value::as_str)
            .map(String::from);
        let args_delta = tc
            .get("function")
            .and_then(|f| f.get("arguments"))
            .and_then(Value::as_str)
            .map(String::from);

        let slot = self.slot(index);
        if let Some(id) = new_id {
            slot.id = Some(id);
        }
        if let Some(name) = new_name {
            slot.name = Some(name);
        }

        if !slot.started && slot.name.is_some() {
            let stable_id = slot.id.clone().unwrap_or_else(|| format!("call_{}", index));
            let name = slot.name.clone().expect("checked above");
            slot.stable_id = stable_id.clone();
            slot.started = true;
            events.push(GenerateStreamEvent::ToolCallStart {
                id: stable_id,
                name,
            });
        }

        if let Some(args) = args_delta {
            slot.input.push_str(&args);
            if slot.started {
                events.push(GenerateStreamEvent::ToolCallDelta {
                    id: slot.stable_id.clone(),
                    partial_input: args,
                });
            }
        }

        events
    }

    pub(crate) fn drain_ends(&self) -> Vec<GenerateStreamEvent> {
        self.by_index
            .iter()
            .filter_map(|(_, state)| {
                state.started.then(|| GenerateStreamEvent::ToolCallEnd {
                    id: state.stable_id.clone(),
                })
            })
            .collect()
    }

    pub(crate) fn finish(self) -> Result<Vec<ToolCallBlock>, String> {
        self.by_index
            .into_iter()
            .map(|(index, state)| {
                let id = if state.stable_id.is_empty() {
                    state.id.unwrap_or_else(|| format!("call_{}", index))
                } else {
                    state.stable_id
                };
                let name = state.name.unwrap_or_default();
                let input = if state.input.is_empty() {
                    "{}".to_string()
                } else {
                    state.input
                };
                serde_json::from_str::<Value>(&input).map_err(|error| {
                    format!("streamed tool call '{name}' produced invalid JSON input: {error}")
                })?;
                Ok(ToolCallBlock {
                    id,
                    name,
                    input,
                    state: ToolCallState::Submitted,
                })
            })
            .collect()
    }

    fn slot(&mut self, index: u32) -> &mut PendingTool {
        let pos = self.by_index.iter().position(|(i, _)| *i == index);
        match pos {
            Some(pos) => &mut self.by_index[pos].1,
            None => {
                self.by_index.push((
                    index,
                    PendingTool {
                        id: None,
                        name: None,
                        input: String::new(),
                        started: false,
                        stable_id: String::new(),
                    },
                ));
                &mut self.by_index.last_mut().expect("just pushed").1
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn accumulates_openai_chat_tool_call() {
        let mut stream = ToolStream::new();
        let mut events = stream.append_openai_chat_delta(&json!({
            "index": 0,
            "id": "call_1",
            "function": { "name": "read", "arguments": "{\"path\":" }
        }));
        events.extend(stream.append_openai_chat_delta(&json!({
            "index": 0,
            "function": { "arguments": "\"Cargo.toml\"}" }
        })));

        assert!(matches!(
            events[0],
            GenerateStreamEvent::ToolCallStart { .. }
        ));
        assert_eq!(stream.drain_ends().len(), 1);
        let calls = stream.finish().unwrap();
        assert_eq!(calls[0].input, "{\"path\":\"Cargo.toml\"}");
    }

    #[test]
    fn rejects_invalid_final_json() {
        let mut stream = ToolStream::new();
        stream.append_openai_chat_delta(&json!({
            "index": 0,
            "id": "call_1",
            "function": { "name": "read", "arguments": "not json" }
        }));

        assert!(stream.finish().is_err());
    }
}
