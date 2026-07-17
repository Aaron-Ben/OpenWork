#[cfg(test)]
use crate::model::ToolCallState;
use crate::model::{FinishReason, ModelEvent, ModelResponse, TokenUsage, ToolCallBlock};
use serde_json::Value;

pub(crate) struct ResponseAccumulator {
    response_id: Option<String>,
    model: Option<String>,
    text: String,
    reasoning_text: String,
    finish_reason: Option<FinishReason>,
    raw_finish_reason: Option<String>,
    usage: Option<TokenUsage>,
}

impl ResponseAccumulator {
    pub(crate) fn new() -> Self {
        Self {
            response_id: None,
            model: None,
            text: String::new(),
            reasoning_text: String::new(),
            finish_reason: None,
            raw_finish_reason: None,
            usage: None,
        }
    }

    pub(crate) fn observe(&mut self, event: &Value) -> (Vec<ModelEvent>, bool) {
        if self.response_id.is_none() {
            self.response_id = event
                .get("id")
                .and_then(Value::as_str)
                .map(ToOwned::to_owned);
        }
        if self.model.is_none() {
            self.model = event
                .get("model")
                .and_then(Value::as_str)
                .map(ToOwned::to_owned);
        }
        if let Some(usage) = usage_from_openai(event) {
            self.usage = Some(usage);
        }

        let mut events = Vec::new();
        if let Some(delta) = text_delta(event) {
            self.text.push_str(&delta);
            events.push(ModelEvent::TextDelta { index: 0, delta });
        }
        if let Some(delta) = reasoning_delta(event) {
            self.reasoning_text.push_str(&delta);
            events.push(ModelEvent::ReasoningDelta { index: 1, delta });
        }

        let raw_finish_reason = raw_finish_reason_from_openai(event);
        let terminal = raw_finish_reason.is_some();
        if terminal {
            self.finish_reason = Some(finish_reason_from_openai(event));
            self.raw_finish_reason = raw_finish_reason;
        }
        (events, terminal)
    }

    pub(crate) fn finish(
        self,
        provider_request_id: Option<String>,
        fallback_model: String,
        tool_calls: Vec<ToolCallBlock>,
    ) -> ModelResponse {
        let default_finish_reason = if tool_calls.is_empty() {
            FinishReason::Stop
        } else {
            FinishReason::ToolUse
        };
        ModelResponse {
            response_id: self.response_id,
            provider_request_id,
            model: self.model.or(Some(fallback_model)),
            text: self.text,
            reasoning_text: (!self.reasoning_text.is_empty()).then_some(self.reasoning_text),
            tool_calls,
            provider_opaque_blocks: Vec::new(),
            finish_reason: self.finish_reason.unwrap_or(default_finish_reason),
            raw_finish_reason: self.raw_finish_reason,
            usage: self.usage,
        }
    }
}

pub(crate) fn usage_from_openai(value: &Value) -> Option<TokenUsage> {
    let usage = value.get("usage")?;
    Some(TokenUsage {
        input_tokens: usage
            .get("input_tokens")
            .or_else(|| usage.get("prompt_tokens"))
            .and_then(Value::as_u64),
        output_tokens: usage
            .get("output_tokens")
            .or_else(|| usage.get("completion_tokens"))
            .and_then(Value::as_u64),
        total_tokens: usage.get("total_tokens").and_then(Value::as_u64),
        cached_input_tokens: usage
            .pointer("/prompt_tokens_details/cached_tokens")
            .or_else(|| usage.pointer("/input_tokens_details/cached_tokens"))
            .and_then(Value::as_u64),
        reasoning_tokens: usage
            .pointer("/completion_tokens_details/reasoning_tokens")
            .or_else(|| usage.pointer("/output_tokens_details/reasoning_tokens"))
            .and_then(Value::as_u64),
    })
}

fn raw_finish_reason_from_openai(value: &Value) -> Option<String> {
    value
        .pointer("/choices/0/finish_reason")
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
}

fn finish_reason_from_openai(value: &Value) -> FinishReason {
    match value
        .pointer("/choices/0/finish_reason")
        .and_then(Value::as_str)
    {
        Some("stop") => FinishReason::Stop,
        Some("length") => FinishReason::Length,
        Some("tool_calls") | Some("function_call") => FinishReason::ToolUse,
        Some("content_filter") => FinishReason::ContentFilter,
        Some("refusal") => FinishReason::Refusal,
        Some("insufficient_system_resource") => FinishReason::Incomplete,
        Some(other) => FinishReason::Unknown(other.to_string()),
        None => FinishReason::Stop,
    }
}

fn text_delta(event: &Value) -> Option<String> {
    event
        .pointer("/choices/0/delta/content")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

fn reasoning_delta(event: &Value) -> Option<String> {
    event
        .pointer("/choices/0/delta")
        .and_then(|delta| {
            delta
                .get("reasoning_content")
                .or_else(|| delta.get("reasoning"))
        })
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

#[cfg(test)]
pub(crate) fn parse_buffered(raw: Value) -> ModelResponse {
    let text = response_text(&raw);
    let reasoning_text = raw
        .pointer("/choices/0/message/reasoning_content")
        .and_then(Value::as_str)
        .map(ToOwned::to_owned);
    ModelResponse {
        response_id: raw.get("id").and_then(Value::as_str).map(ToOwned::to_owned),
        provider_request_id: None,
        model: raw
            .get("model")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned),
        text,
        reasoning_text,
        tool_calls: tool_calls(&raw),
        provider_opaque_blocks: Vec::new(),
        finish_reason: finish_reason_from_openai(&raw),
        raw_finish_reason: raw_finish_reason_from_openai(&raw),
        usage: usage_from_openai(&raw),
    }
}

#[cfg(test)]
fn response_text(value: &Value) -> String {
    let Some(content) = value.pointer("/choices/0/message/content") else {
        return String::new();
    };
    match content {
        Value::String(text) => text.clone(),
        Value::Array(parts) => parts
            .iter()
            .filter_map(|part| part.get("text").and_then(Value::as_str))
            .collect::<Vec<_>>()
            .join(""),
        _ => String::new(),
    }
}

#[cfg(test)]
fn tool_calls(value: &Value) -> Vec<ToolCallBlock> {
    value
        .pointer("/choices/0/message/tool_calls")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|call| {
            let function = call.get("function")?;
            Some(ToolCallBlock {
                id: call.get("id")?.as_str()?.to_string(),
                name: function.get("name")?.as_str()?.to_string(),
                input: function
                    .get("arguments")
                    .and_then(Value::as_str)
                    .unwrap_or("{}")
                    .to_string(),
                state: ToolCallState::Submitted,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_chat_completion_text_reasoning_and_usage() {
        let raw = json!({
            "choices": [{
                "message": { "reasoning_content": "thinking", "content": "answer" }
            }],
            "usage": { "prompt_tokens": 2, "completion_tokens": 3, "total_tokens": 5 }
        });

        let response = parse_buffered(raw);

        assert_eq!(response.text, "answer");
        assert_eq!(response.reasoning_text.as_deref(), Some("thinking"));
        assert_eq!(response.usage.unwrap().output_tokens, Some(3));
    }

    #[test]
    fn extracts_stream_text_and_reasoning_deltas() {
        let raw = json!({
            "choices": [{ "delta": { "reasoning_content": "think", "content": "answer" } }]
        });
        let mut response = ResponseAccumulator::new();

        let (events, terminal) = response.observe(&raw);

        assert!(!terminal);
        assert!(matches!(
            events.as_slice(),
            [ModelEvent::TextDelta { delta: text, .. }, ModelEvent::ReasoningDelta { delta: reasoning, .. }]
                if text == "answer" && reasoning == "think"
        ));
    }

    #[test]
    fn parses_tool_calls_from_completion() {
        let raw = json!({
            "choices": [{ "message": { "tool_calls": [{
                "id": "call_abc",
                "function": { "name": "read", "arguments": "{\"path\":\"Cargo.toml\"}" }
            }] } }]
        });

        let response = parse_buffered(raw);

        assert_eq!(response.tool_calls.len(), 1);
        assert_eq!(response.tool_calls[0].name, "read");
    }

    #[test]
    fn maps_deepseek_resource_interrupt_to_incomplete() {
        let response = json!({
            "choices": [{ "finish_reason": "insufficient_system_resource" }]
        });

        assert_eq!(
            finish_reason_from_openai(&response),
            FinishReason::Incomplete
        );
    }
}
