//! OpenAI Responses stream event codec boundary.

use std::{collections::VecDeque, pin::Pin};

use futures_util::{Stream, StreamExt, stream};
use openwork_protocol::model::{ModelError, ModelEvent, ModelStream, ToolCallBlock, ToolCallState};
use serde_json::Value;

use super::response::ResponseAccumulator;
use crate::{
    error::{ErrorDialect, decode_stream_json, map_stream_error_event_for},
    sse::{SseFrame, sse_frames},
};

struct OpenAiResponseStreamState<F> {
    frames: Pin<Box<F>>,
    accumulator: Option<ResponseAccumulator>,
    tools: Option<OpenAiResponsesToolStream>,
    pending: VecDeque<ModelEvent>,
    provider_request_id: Option<String>,
    fallback_model: String,
    terminal: bool,
    completed: bool,
}

pub(crate) fn response_stream(
    response: reqwest::Response,
    provider_request_id: Option<String>,
    fallback_model: String,
) -> ModelStream {
    let state = OpenAiResponseStreamState {
        frames: Box::pin(sse_frames(response)),
        accumulator: Some(ResponseAccumulator::default()),
        tools: Some(OpenAiResponsesToolStream::default()),
        pending: VecDeque::new(),
        provider_request_id,
        fallback_model,
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
                    .ok_or_else(|| ModelError::protocol("OpenAI tool stream already finished"))?;
                let (tool_calls, tool_end_events) = tools.finish().map_err(ModelError::protocol)?;
                state.pending.extend(tool_end_events);
                let accumulator = state.accumulator.take().ok_or_else(|| {
                    ModelError::protocol("OpenAI response accumulator already finished")
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
            let event = decode_stream_json(&frame.data)?;
            if let Some(error) = map_stream_error_event_for(&event, ErrorDialect::OpenAi) {
                return Err(error);
            }
            let accumulator = state.accumulator.as_mut().ok_or_else(|| {
                ModelError::protocol("OpenAI response accumulator is unavailable")
            })?;
            let (events, terminal) = accumulator.observe(&event);
            state.pending.extend(events);
            let tools = state
                .tools
                .as_mut()
                .ok_or_else(|| ModelError::protocol("OpenAI tool stream is unavailable"))?;
            state.pending.extend(tools.observe(&event));
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
            "OpenAI stream ended before a terminal event",
        )),
    }
}

#[derive(Debug)]
struct PendingTool {
    index: u32,
    item_id: String,
    call_id: String,
    name: String,
    input: String,
}

#[derive(Debug, Default)]
pub(crate) struct OpenAiResponsesToolStream {
    tools: Vec<PendingTool>,
}

impl OpenAiResponsesToolStream {
    pub(crate) fn observe(&mut self, event: &Value) -> Vec<ModelEvent> {
        let index = event
            .get("output_index")
            .and_then(Value::as_u64)
            .unwrap_or(0) as u32;
        match event.get("type").and_then(Value::as_str) {
            Some("response.output_item.added")
                if event.pointer("/item/type").and_then(Value::as_str) == Some("function_call") =>
            {
                let Some(item_id) = event.pointer("/item/id").and_then(Value::as_str) else {
                    return Vec::new();
                };
                let call_id = event
                    .pointer("/item/call_id")
                    .and_then(Value::as_str)
                    .unwrap_or(item_id);
                let Some(name) = event.pointer("/item/name").and_then(Value::as_str) else {
                    return Vec::new();
                };
                let input = event
                    .pointer("/item/arguments")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string();
                self.tools.push(PendingTool {
                    index,
                    item_id: item_id.to_string(),
                    call_id: call_id.to_string(),
                    name: name.to_string(),
                    input,
                });
                vec![ModelEvent::ToolCallStart {
                    index,
                    id: call_id.to_string(),
                    name: name.to_string(),
                }]
            }
            Some("response.function_call_arguments.delta") => {
                let Some(delta) = event.get("delta").and_then(Value::as_str) else {
                    return Vec::new();
                };
                let item_id = event.get("item_id").and_then(Value::as_str);
                let Some(tool) = self.tools.iter_mut().find(|tool| {
                    tool.index == index && item_id.is_none_or(|id| id == tool.item_id)
                }) else {
                    return Vec::new();
                };
                tool.input.push_str(delta);
                vec![ModelEvent::ToolCallDelta {
                    index,
                    id: tool.call_id.clone(),
                    partial_input: delta.to_string(),
                }]
            }
            Some("response.function_call_arguments.done") => {
                let item_id = event.get("item_id").and_then(Value::as_str);
                let Some(tool) = self.tools.iter_mut().find(|tool| {
                    tool.index == index && item_id.is_none_or(|id| id == tool.item_id)
                }) else {
                    return Vec::new();
                };
                if let Some(arguments) = event.get("arguments").and_then(Value::as_str)
                    && tool.input.is_empty()
                {
                    tool.input = arguments.to_string();
                }
                Vec::new()
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
                    "OpenAI Responses tool call '{}' produced invalid JSON input: {error}",
                    tool.name
                )
            })?;
            ends.push(ModelEvent::ToolCallEnd {
                index: tool.index,
                id: tool.call_id.clone(),
            });
            calls.push(ToolCallBlock {
                id: tool.call_id,
                name: tool.name,
                input,
                state: ToolCallState::Submitted,
            });
        }
        Ok((calls, ends))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use openwork_protocol::model::ModelEvent;
    use serde_json::json;

    #[test]
    fn accumulates_function_call_argument_events() {
        let mut stream = OpenAiResponsesToolStream::default();
        let started = stream.observe(&json!({
            "type": "response.output_item.added",
            "output_index": 2,
            "item": {
                "type": "function_call",
                "id": "fc_1",
                "call_id": "call_1",
                "name": "read",
                "arguments": ""
            }
        }));
        let delta = stream.observe(&json!({
            "type": "response.function_call_arguments.delta",
            "output_index": 2,
            "item_id": "fc_1",
            "delta": "{\"path\":\"Cargo.toml\"}"
        }));

        assert!(matches!(
            started[0],
            ModelEvent::ToolCallStart { index: 2, .. }
        ));
        assert!(matches!(
            delta[0],
            ModelEvent::ToolCallDelta { index: 2, .. }
        ));
        let (calls, ends) = stream.finish().unwrap();
        assert_eq!(calls[0].id, "call_1");
        assert_eq!(calls[0].input, r#"{"path":"Cargo.toml"}"#);
        assert!(matches!(ends[0], ModelEvent::ToolCallEnd { index: 2, .. }));
    }
}
