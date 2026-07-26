use crate::model::{
    FinishReason, ModelEvent, ModelResponse, ProviderOpaqueBlock, TokenUsage, ToolCallBlock,
};
#[cfg(test)]
use crate::provider::ProviderDriver;
use serde_json::Value;

pub(crate) struct ResponseAccumulator {
    response_id: Option<String>,
    model: Option<String>,
    text: String,
    reasoning_text: String,
    finish_reason: FinishReason,
    raw_finish_reason: Option<String>,
    usage: Option<TokenUsage>,
}

impl Default for ResponseAccumulator {
    fn default() -> Self {
        Self {
            response_id: None,
            model: None,
            text: String::new(),
            reasoning_text: String::new(),
            finish_reason: FinishReason::Stop,
            raw_finish_reason: None,
            usage: None,
        }
    }
}

impl ResponseAccumulator {
    pub(crate) fn observe(&mut self, event: &Value) -> (Vec<ModelEvent>, bool) {
        let mut events = Vec::new();
        match event.get("type").and_then(Value::as_str) {
            Some("message_start") => {
                self.response_id = event
                    .pointer("/message/id")
                    .and_then(Value::as_str)
                    .map(ToOwned::to_owned);
                self.model = event
                    .pointer("/message/model")
                    .and_then(Value::as_str)
                    .map(ToOwned::to_owned);
                self.usage = merge_usage(event.pointer("/message/usage"), self.usage.take());
            }
            Some("message_delta") => {
                self.usage = merge_usage(event.get("usage"), self.usage.take());
            }
            _ => {}
        }

        if let Some(delta) = text_delta(event) {
            self.text.push_str(&delta);
            events.push(ModelEvent::TextDelta { index: 0, delta });
        }
        if let Some(delta) = reasoning_delta(event) {
            self.reasoning_text.push_str(&delta);
            events.push(ModelEvent::ReasoningDelta { index: 1, delta });
        }
        if let Some(reason) = event.pointer("/delta/stop_reason").and_then(Value::as_str) {
            self.finish_reason = finish_reason(Some(reason));
            self.raw_finish_reason = Some(reason.to_string());
        }

        let terminal = event.get("type").and_then(Value::as_str) == Some("message_stop");
        (events, terminal)
    }

    pub(crate) fn finish(
        self,
        provider_request_id: Option<String>,
        fallback_model: String,
        tool_calls: Vec<ToolCallBlock>,
        provider_opaque_blocks: Vec<ProviderOpaqueBlock>,
    ) -> ModelResponse {
        ModelResponse {
            response_id: self.response_id,
            provider_request_id,
            model: self.model.or(Some(fallback_model)),
            text: self.text,
            reasoning_text: (!self.reasoning_text.is_empty()).then_some(self.reasoning_text),
            tool_calls,
            provider_opaque_blocks,
            finish_reason: self.finish_reason,
            raw_finish_reason: self.raw_finish_reason,
            usage: self.usage,
        }
    }
}

fn finish_reason(reason: Option<&str>) -> FinishReason {
    match reason {
        Some("end_turn") | Some("stop_sequence") => FinishReason::Stop,
        Some("max_tokens") => FinishReason::Length,
        Some("tool_use") => FinishReason::ToolUse,
        Some("refusal") => FinishReason::Refusal,
        Some(other) => FinishReason::Unknown(other.to_string()),
        None => FinishReason::Stop,
    }
}

fn merge_usage(value: Option<&Value>, existing: Option<TokenUsage>) -> Option<TokenUsage> {
    let value = value?;
    let existing = existing.unwrap_or(TokenUsage {
        input_tokens: None,
        output_tokens: None,
        total_tokens: None,
        cached_input_tokens: None,
        cache_creation_input_tokens: None,
        reasoning_tokens: None,
    });
    let input_tokens = value
        .get("input_tokens")
        .and_then(Value::as_u64)
        .or(existing.input_tokens);
    let output_tokens = value
        .get("output_tokens")
        .and_then(Value::as_u64)
        .or(existing.output_tokens);
    Some(TokenUsage {
        input_tokens,
        output_tokens,
        total_tokens: match (input_tokens, output_tokens) {
            (Some(input), Some(output)) => Some(input.saturating_add(output)),
            _ => None,
        },
        cached_input_tokens: value
            .get("cache_read_input_tokens")
            .and_then(Value::as_u64)
            .or(existing.cached_input_tokens),
        cache_creation_input_tokens: value
            .get("cache_creation_input_tokens")
            .and_then(Value::as_u64)
            .or(existing.cache_creation_input_tokens),
        reasoning_tokens: None,
    })
}

fn text_delta(event: &Value) -> Option<String> {
    let delta = event.get("delta")?;
    match delta.get("type").and_then(Value::as_str) {
        Some("text_delta") => delta
            .get("text")
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned),
        _ => None,
    }
}

fn reasoning_delta(event: &Value) -> Option<String> {
    let delta = event.get("delta")?;
    match delta.get("type").and_then(Value::as_str) {
        Some("thinking_delta") => delta
            .get("thinking")
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned),
        _ => None,
    }
}

#[cfg(test)]
pub(crate) fn parse_buffered(raw: Value) -> ModelResponse {
    let text = raw
        .get("content")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|part| match part.get("type").and_then(Value::as_str) {
            Some("text") => part.get("text").and_then(Value::as_str),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("");
    let usage = merge_usage(raw.get("usage"), None);

    ModelResponse {
        response_id: raw.get("id").and_then(Value::as_str).map(ToOwned::to_owned),
        provider_request_id: None,
        model: raw
            .get("model")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned),
        text,
        reasoning_text: None,
        tool_calls: Vec::new(),
        provider_opaque_blocks: opaque_blocks(&raw),
        finish_reason: finish_reason(raw.get("stop_reason").and_then(Value::as_str)),
        raw_finish_reason: raw
            .get("stop_reason")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned),
        usage,
    }
}

#[cfg(test)]
fn opaque_blocks(raw: &Value) -> Vec<ProviderOpaqueBlock> {
    raw.get("content")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|block| {
            let kind = block.get("type").and_then(Value::as_str)?;
            if !matches!(kind, "thinking" | "redacted_thinking") {
                return None;
            }
            if serde_json::to_vec(block).ok()?.len() > 64 * 1024 {
                return None;
            }
            Some(ProviderOpaqueBlock {
                driver: ProviderDriver::AnthropicMessages,
                kind: kind.to_string(),
                payload: block.clone(),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_message_text_and_usage() {
        let raw = json!({
            "content": [
                { "type": "text", "text": "hello" },
                { "type": "text", "text": " world" }
            ],
            "usage": { "input_tokens": 2, "output_tokens": 3 }
        });

        let response = parse_buffered(raw);

        assert_eq!(response.text, "hello world");
        assert_eq!(response.usage.unwrap().total_tokens, Some(5));
    }

    #[test]
    fn extracts_anthropic_stream_deltas() {
        let mut response = ResponseAccumulator::default();
        let text = json!({ "delta": { "type": "text_delta", "text": "hello" } });
        let reasoning = json!({ "delta": { "type": "thinking_delta", "thinking": "think" } });

        let (text_events, _) = response.observe(&text);
        let (reasoning_events, _) = response.observe(&reasoning);

        assert!(matches!(
            text_events.as_slice(),
            [ModelEvent::TextDelta { delta, .. }] if delta == "hello"
        ));
        assert!(matches!(
            reasoning_events.as_slice(),
            [ModelEvent::ReasoningDelta { delta, .. }] if delta == "think"
        ));
    }
}
