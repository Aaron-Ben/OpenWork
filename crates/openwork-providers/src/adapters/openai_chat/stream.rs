use std::{collections::VecDeque, pin::Pin};

use futures_util::{Stream, StreamExt, stream};
use openwork_protocol::model::{ModelError, ModelEvent, ModelStream, ToolCallBlock, ToolCallState};
use serde_json::Value;

use super::response::ResponseAccumulator;
use crate::{
    error::{ErrorDialect, decode_stream_json, map_stream_error_event_for},
    sse::{SseFrame, sse_frames},
};

struct ChatResponseStreamState<F> {
    frames: Pin<Box<F>>,
    accumulator: Option<ResponseAccumulator>,
    tools: Option<ToolStream>,
    pending: VecDeque<ModelEvent>,
    provider_request_id: Option<String>,
    fallback_model: String,
    dialect: ErrorDialect,
    terminal: bool,
    completed: bool,
}

pub(crate) fn response_stream(
    response: reqwest::Response,
    provider_request_id: Option<String>,
    fallback_model: String,
    dialect: ErrorDialect,
) -> ModelStream {
    let state = ChatResponseStreamState {
        frames: Box::pin(sse_frames(response)),
        accumulator: Some(ResponseAccumulator::new()),
        tools: Some(ToolStream::new()),
        pending: VecDeque::new(),
        provider_request_id,
        fallback_model,
        dialect,
        terminal: false,
        completed: false,
    };

    Box::pin(stream::try_unfold(state, |mut state| async move {
        loop {
            if let Some(event) = state.pending.pop_front() {
                return Ok(Some((event, state)));
            }
            if state.completed {
                return Ok(None);
            }
            if state.terminal {
                let tools = state
                    .tools
                    .take()
                    .ok_or_else(|| ModelError::protocol("chat tool stream already finished"))?;
                state.pending.extend(tools.drain_ends());
                let tool_calls = tools.finish().map_err(ModelError::invalid_request)?;
                let accumulator = state.accumulator.take().ok_or_else(|| {
                    ModelError::protocol("chat response accumulator already finished")
                })?;
                let response = accumulator.finish(
                    state.provider_request_id.take(),
                    std::mem::take(&mut state.fallback_model),
                    tool_calls,
                );
                state.pending.push_back(ModelEvent::ResponseCompleted {
                    response: Box::new(response),
                });
                state.completed = true;
                continue;
            }

            let frame = next_frame(&mut state.frames).await?;
            if frame.data == "[DONE]" {
                state.terminal = true;
                continue;
            }
            let event = decode_stream_json(&frame.data)?;
            if let Some(error) = map_stream_error_event_for(&event, state.dialect) {
                return Err(error);
            }
            let accumulator = state
                .accumulator
                .as_mut()
                .ok_or_else(|| ModelError::protocol("chat response accumulator is unavailable"))?;
            let (events, terminal) = accumulator.observe(&event);
            state.pending.extend(events);
            if let Some(tool_call_deltas) = event
                .pointer("/choices/0/delta/tool_calls")
                .and_then(Value::as_array)
            {
                let tools = state
                    .tools
                    .as_mut()
                    .ok_or_else(|| ModelError::protocol("chat tool stream is unavailable"))?;
                for tool_call in tool_call_deltas {
                    state
                        .pending
                        .extend(tools.append_openai_chat_delta(tool_call));
                }
            }
            state.terminal = terminal;
        }
    }))
}

async fn next_frame<F>(frames: &mut Pin<Box<F>>) -> Result<SseFrame, ModelError>
where
    F: Stream<Item = Result<SseFrame, ModelError>>,
{
    match frames.next().await {
        Some(frame) => frame,
        None => Err(ModelError::network(
            "chat stream ended before a terminal event",
        )),
    }
}

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

    pub(crate) fn append_openai_chat_delta(&mut self, tc: &Value) -> Vec<ModelEvent> {
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
            events.push(ModelEvent::ToolCallStart {
                index,
                id: stable_id,
                name,
            });
        }

        if let Some(args) = args_delta {
            slot.input.push_str(&args);
            if slot.started {
                events.push(ModelEvent::ToolCallDelta {
                    index,
                    id: slot.stable_id.clone(),
                    partial_input: args,
                });
            }
        }

        events
    }

    pub(crate) fn drain_ends(&self) -> Vec<ModelEvent> {
        self.by_index
            .iter()
            .filter(|(_, state)| state.started)
            .map(|(index, state)| ModelEvent::ToolCallEnd {
                index: *index,
                id: state.stable_id.clone(),
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

        assert!(matches!(events[0], ModelEvent::ToolCallStart { .. }));
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
