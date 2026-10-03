//! Responses 的流事件累加器：文本、思考、reasoning 条目、用量与结束原因。

use crate::model::{
    FinishReason, ModelEvent, ModelResponse, ProviderOpaqueBlock, TokenUsage, ToolCallBlock,
};
use serde_json::Value;

#[derive(Debug)]
pub(crate) struct ResponseAccumulator {
    response_id: Option<String>,
    model: Option<String>,
    text: String,
    reasoning_text: String,
    /// 原样保存的 reasoning 条目，下一次请求时回传。
    reasoning_items: Vec<Value>,
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
            reasoning_items: Vec::new(),
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
            if let Some(usage) = usage_from_response(response) {
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
        if event_type == Some("response.output_item.done")
            && event.pointer("/item/type").and_then(Value::as_str) == Some("reasoning")
            && let Some(item) = event.get("item")
        {
            self.reasoning_items.push(item.clone());
        }
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
            provider_opaque_blocks: self
                .reasoning_items
                .into_iter()
                .map(|payload| ProviderOpaqueBlock {
                    kind: super::request::REASONING_ITEM_KIND.to_string(),
                    payload,
                })
                .collect(),
            finish_reason: self.finish_reason,
            raw_finish_reason: self.raw_finish_reason,
            usage: self.usage,
        }
    }
}

/// Responses 的用量字段（`response.usage`）。
fn usage_from_response(response: &Value) -> Option<TokenUsage> {
    let usage = response.get("usage")?;
    Some(TokenUsage {
        input_tokens: usage.get("input_tokens").and_then(Value::as_u64),
        output_tokens: usage.get("output_tokens").and_then(Value::as_u64),
        total_tokens: usage.get("total_tokens").and_then(Value::as_u64),
        cached_input_tokens: usage
            .pointer("/input_tokens_details/cached_tokens")
            .and_then(Value::as_u64),
        cache_creation_input_tokens: None,
        reasoning_tokens: usage
            .pointer("/output_tokens_details/reasoning_tokens")
            .and_then(Value::as_u64),
    })
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
        usage: usage_from_response(&raw),
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
    fn keeps_reasoning_items_for_replay_and_reads_usage() {
        let mut accumulator = ResponseAccumulator::default();
        let reasoning = json!({ "type": "reasoning", "id": "rs_1", "summary": [], "encrypted_content": "gAAA" });
        accumulator.observe(&json!({ "type": "response.output_item.done", "item": reasoning }));
        let (_, terminal) = accumulator.observe(&json!({
            "type": "response.completed",
            "response": {
                "id": "resp_1",
                "usage": {
                    "input_tokens": 10,
                    "output_tokens": 5,
                    "total_tokens": 15,
                    "input_tokens_details": { "cached_tokens": 4 },
                    "output_tokens_details": { "reasoning_tokens": 3 }
                }
            }
        }));
        assert!(terminal);
        let response = accumulator.finish(None, "m".to_string(), Vec::new());

        assert_eq!(response.provider_opaque_blocks.len(), 1);
        assert_eq!(response.provider_opaque_blocks[0].kind, "reasoning");
        assert_eq!(response.provider_opaque_blocks[0].payload, reasoning);
        let usage = response.usage.unwrap();
        assert_eq!(usage.cached_input_tokens, Some(4));
        assert_eq!(usage.reasoning_tokens, Some(3));
    }

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
