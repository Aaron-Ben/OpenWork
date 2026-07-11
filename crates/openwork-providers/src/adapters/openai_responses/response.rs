//! OpenAI Responses response and stream-event accumulator.

use openwork_protocol::model::{
    FinishReason, ModelEvent, ModelResponse, TokenUsage, ToolCallBlock,
};
use serde_json::Value;

#[derive(Debug)]
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
        if let Some(response) = event.get("response") {
            self.response_id = self.response_id.take().or_else(|| {
                response
                    .get("id")
                    .and_then(Value::as_str)
                    .map(ToOwned::to_owned)
            });
            self.model = self.model.take().or_else(|| {
                response
                    .get("model")
                    .and_then(Value::as_str)
                    .map(ToOwned::to_owned)
            });
            if let Some(usage) = crate::adapters::openai_chat::response::usage_from_openai(response)
            {
                self.usage = Some(usage);
            }
        }

        let mut normalized = Vec::new();
        if let Some(delta) = text_delta(event) {
            self.text.push_str(&delta);
            normalized.push(ModelEvent::TextDelta { index: 0, delta });
        }
        if let Some(delta) = reasoning_delta(event) {
            self.reasoning_text.push_str(&delta);
            normalized.push(ModelEvent::ReasoningDelta { index: 1, delta });
        }

        let event_type = event.get("type").and_then(Value::as_str);
        if event_type == Some("response.refusal.delta") {
            self.finish_reason = FinishReason::Refusal;
            self.raw_finish_reason = Some("refusal".to_string());
        }
        if event_type == Some("response.incomplete") {
            self.raw_finish_reason = event
                .pointer("/response/incomplete_details/reason")
                .and_then(Value::as_str)
                .map(ToOwned::to_owned);
            self.finish_reason = match self.raw_finish_reason.as_deref() {
                Some("max_output_tokens") => FinishReason::Length,
                Some("content_filter") => FinishReason::ContentFilter,
                _ => FinishReason::Incomplete,
            };
        } else if event_type == Some("response.completed") && self.raw_finish_reason.is_none() {
            self.raw_finish_reason = Some("completed".to_string());
        }

        (
            normalized,
            matches!(
                event_type,
                Some("response.completed" | "response.incomplete" | "response.failed")
            ),
        )
    }

    pub(crate) fn finish(
        mut self,
        provider_request_id: Option<String>,
        fallback_model: String,
        tool_calls: Vec<ToolCallBlock>,
    ) -> ModelResponse {
        if !tool_calls.is_empty() && self.finish_reason == FinishReason::Stop {
            self.finish_reason = FinishReason::ToolUse;
            self.raw_finish_reason = Some("tool_use".to_string());
        }
        ModelResponse {
            response_id: self.response_id,
            provider_request_id,
            model: self.model.or(Some(fallback_model)),
            text: self.text,
            reasoning_text: (!self.reasoning_text.is_empty()).then_some(self.reasoning_text),
            tool_calls,
            provider_opaque_blocks: Vec::new(),
            finish_reason: self.finish_reason,
            raw_finish_reason: self.raw_finish_reason,
            usage: self.usage,
        }
    }
}

fn text_delta(event: &Value) -> Option<String> {
    match event.get("type").and_then(Value::as_str) {
        Some("response.output_text.delta") | Some("response.refusal.delta") => event
            .get("delta")
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned),
        _ => None,
    }
}

fn reasoning_delta(event: &Value) -> Option<String> {
    match event.get("type").and_then(Value::as_str) {
        Some("response.reasoning_text.delta") | Some("response.reasoning_summary_text.delta") => {
            event
                .get("delta")
                .and_then(Value::as_str)
                .filter(|value| !value.is_empty())
                .map(ToOwned::to_owned)
        }
        _ => None,
    }
}

#[cfg(test)]
pub(crate) fn parse_buffered(raw: Value) -> ModelResponse {
    let text = raw
        .get("output_text")
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| output_text(&raw));
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
        provider_opaque_blocks: Vec::new(),
        finish_reason: match raw.get("status").and_then(Value::as_str) {
            Some("incomplete" | "failed") => FinishReason::Incomplete,
            Some("cancelled") => FinishReason::Cancelled,
            _ => FinishReason::Stop,
        },
        raw_finish_reason: raw
            .get("status")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned),
        usage: crate::adapters::openai_chat::response::usage_from_openai(&raw),
    }
}

#[cfg(test)]
fn output_text(raw: &Value) -> String {
    raw.get("output")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|item| item.get("content").and_then(Value::as_array))
        .flatten()
        .filter_map(|part| match part.get("type").and_then(Value::as_str) {
            Some("output_text") => part.get("text").and_then(Value::as_str),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn joins_response_output_text_parts() {
        let raw = json!({
            "output": [{
                "content": [
                    { "type": "output_text", "text": "hello" },
                    { "type": "output_text", "text": " world" }
                ]
            }]
        });
        assert_eq!(parse_buffered(raw).text, "hello world");
    }

    #[test]
    fn extracts_responses_stream_deltas() {
        let mut accumulator = ResponseAccumulator::default();
        let (text, _) =
            accumulator.observe(&json!({ "type": "response.output_text.delta", "delta": "hello" }));
        let (reasoning, _) = accumulator
            .observe(&json!({ "type": "response.reasoning_text.delta", "delta": "think" }));
        assert!(matches!(text[0], ModelEvent::TextDelta { .. }));
        assert!(matches!(reasoning[0], ModelEvent::ReasoningDelta { .. }));
    }
}
