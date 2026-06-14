use anvil_core::ai::{
    GenerateRequest, GenerateResponse, GenerateStreamCallback, GenerateStreamEvent, LlmProvider,
    ProviderError, ToolCallBlock, ToolCallState,
};
use async_trait::async_trait;
use reqwest::header::{AUTHORIZATION, CONTENT_TYPE, HeaderMap, HeaderValue};
use serde_json::{Map, Value, json};

use crate::{
    config::HttpProviderConfig,
    error::{map_error_response, map_reqwest_error},
    serde_helpers::{
        openai_chat_message, openai_chat_tools, reasoning_text_from_chat_completion,
        response_text_from_chat_completion, tool_calls_from_chat_completion, usage_from_openai,
    },
    sse::consume_sse_response,
};

#[derive(Debug, Clone)]
pub struct OpenAiCompatibleChatProvider {
    client: reqwest::Client,
    config: HttpProviderConfig,
    extra_body: Map<String, Value>,
}

impl OpenAiCompatibleChatProvider {
    pub fn new(config: HttpProviderConfig) -> Self {
        Self {
            client: reqwest::Client::new(),
            config,
            extra_body: Map::new(),
        }
    }

    pub fn with_extra_body(mut self, extra_body: Map<String, Value>) -> Self {
        self.extra_body = extra_body;
        self
    }

    pub fn kimi(api_key: impl Into<String>) -> Self {
        Self::new(HttpProviderConfig::new("https://api.moonshot.cn", api_key))
    }

    pub fn deepseek(api_key: impl Into<String>) -> Self {
        Self::new(HttpProviderConfig::new("https://api.deepseek.com", api_key))
    }

    pub fn qwen_dashscope(api_key: impl Into<String>) -> Self {
        Self::new(HttpProviderConfig::new(
            "https://dashscope.aliyuncs.com/compatible-mode/v1",
            api_key,
        ))
    }

    fn headers(&self) -> Result<HeaderMap, ProviderError> {
        let mut headers = HeaderMap::new();
        let auth =
            HeaderValue::from_str(&format!("Bearer {}", self.config.api_key)).map_err(|error| {
                ProviderError::InvalidRequest {
                    message: error.to_string(),
                }
            })?;
        headers.insert(AUTHORIZATION, auth);
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        Ok(headers)
    }

    pub(crate) fn chat_completions_request_body(
        &self,
        req: &GenerateRequest,
    ) -> Result<Value, ProviderError> {
        if req.thinking.is_some() {
            return Err(ProviderError::InvalidRequest {
                message: "thinking mode is provider-specific; use a dedicated provider adapter"
                    .to_string(),
            });
        }

        let messages = req
            .messages
            .iter()
            .map(openai_chat_message)
            .collect::<Result<Vec<_>, _>>()?;

        let mut body = Map::new();
        body.insert("model".to_string(), json!(req.model));
        body.insert("messages".to_string(), json!(messages));
        body.insert("stream".to_string(), json!(req.stream));

        if let Some(temperature) = req.temperature {
            body.insert("temperature".to_string(), json!(temperature));
        }
        if let Some(max_tokens) = req.max_tokens {
            body.insert("max_tokens".to_string(), json!(max_tokens));
        }

        if !req.tools.is_empty() {
            body.insert("tools".to_string(), openai_chat_tools(&req.tools));
            body.insert("tool_choice".to_string(), json!("auto"));
        }

        for (key, value) in &self.extra_body {
            body.insert(key.clone(), value.clone());
        }

        Ok(Value::Object(body))
    }

    pub(crate) fn parse_generate_response(raw: Value) -> GenerateResponse {
        let text = response_text_from_chat_completion(&raw);
        let reasoning_text = reasoning_text_from_chat_completion(&raw);
        let usage = usage_from_openai(&raw);
        let tool_calls = tool_calls_from_chat_completion(&raw);
        GenerateResponse {
            text,
            reasoning_text,
            usage,
            raw,
            tool_calls,
        }
    }
}

#[async_trait]
impl LlmProvider for OpenAiCompatibleChatProvider {
    async fn generate(&self, req: GenerateRequest) -> Result<GenerateResponse, ProviderError> {
        if req.stream {
            return Err(ProviderError::InvalidRequest {
                message: "streaming is not implemented for OpenAI-compatible providers yet"
                    .to_string(),
            });
        }

        let body = self.chat_completions_request_body(&req)?;
        let response = self
            .client
            .post(self.config.endpoint("/chat/completions"))
            .headers(self.headers()?)
            .json(&body)
            .send()
            .await
            .map_err(map_reqwest_error)?;

        if !response.status().is_success() {
            return Err(map_error_response(response).await);
        }

        let raw = response.json::<Value>().await.map_err(map_reqwest_error)?;
        Ok(Self::parse_generate_response(raw))
    }

    async fn stream_generate(
        &self,
        mut req: GenerateRequest,
        mut on_event: GenerateStreamCallback,
    ) -> Result<GenerateResponse, ProviderError> {
        req.stream = true;
        let body = self.chat_completions_request_body(&req)?;
        let response = self
            .client
            .post(self.config.endpoint("/chat/completions"))
            .headers(self.headers()?)
            .json(&body)
            .send()
            .await
            .map_err(map_reqwest_error)?;

        if !response.status().is_success() {
            return Err(map_error_response(response).await);
        }

        let mut text = String::new();
        let mut reasoning_text = String::new();
        let mut acc = ToolCallAccumulator::new();
        let raw_events = consume_sse_response(response, |event| {
            if let Some(delta) = chat_delta(event) {
                text.push_str(&delta);
                on_event(GenerateStreamEvent::TextDelta { delta });
            }
            if let Some(delta) = chat_reasoning_delta(event) {
                reasoning_text.push_str(&delta);
                on_event(GenerateStreamEvent::ReasoningDelta { delta });
            }
            if let Some(tool_call_deltas) = event
                .get("choices")
                .and_then(Value::as_array)
                .and_then(|choices| choices.first())
                .and_then(|choice| choice.get("delta"))
                .and_then(|delta| delta.get("tool_calls"))
                .and_then(Value::as_array)
            {
                for tool_call in tool_call_deltas {
                    for ev in acc.on_delta(tool_call) {
                        on_event(ev);
                    }
                }
            }
            Ok(())
        })
        .await?;

        for ev in acc.drain_ends() {
            on_event(ev);
        }
        let tool_calls = acc.finish();

        Ok(GenerateResponse {
            text,
            reasoning_text: if reasoning_text.is_empty() {
                None
            } else {
                Some(reasoning_text)
            },
            usage: None,
            raw: Value::Array(raw_events),
            tool_calls,
        })
    }
}

pub(crate) fn chat_delta(event: &Value) -> Option<String> {
    event
        .get("choices")
        .and_then(Value::as_array)
        .and_then(|choices| choices.first())
        .and_then(|choice| choice.get("delta"))
        .and_then(|delta| delta.get("content"))
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

pub(crate) fn chat_reasoning_delta(event: &Value) -> Option<String> {
    event
        .get("choices")
        .and_then(Value::as_array)
        .and_then(|choices| choices.first())
        .and_then(|choice| choice.get("delta"))
        .and_then(|delta| {
            delta
                .get("reasoning_content")
                .or_else(|| delta.get("reasoning"))
        })
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

/// 流式 tool call 累积状态:按 OpenAI 的 `index` 跨 chunk 拼接 id/name/arguments。
struct AccumState {
    id: Option<String>,
    name: Option<String>,
    input: String,
    started: bool,
    stable_id: String,
}

struct ToolCallAccumulator {
    by_index: Vec<(u32, AccumState)>,
}

impl ToolCallAccumulator {
    fn new() -> Self {
        Self {
            by_index: Vec::new(),
        }
    }

    fn slot(&mut self, index: u32) -> &mut AccumState {
        let pos = self.by_index.iter().position(|(i, _)| *i == index);
        match pos {
            Some(p) => &mut self.by_index[p].1,
            None => {
                self.by_index.push((
                    index,
                    AccumState {
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

    /// 处理一个 `delta.tool_calls` 项,返回需要 emit 的 Start/Delta 事件。
    fn on_delta(&mut self, tc: &Value) -> Vec<GenerateStreamEvent> {
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

        // 首次 name 已知时 emit Start,并固定 stable_id(优先模型给的 id,否则用 index 生成)。
        if !slot.started && slot.name.is_some() {
            let stable_id = slot
                .id
                .clone()
                .unwrap_or_else(|| format!("call_{}", index));
            let name = slot.name.clone().expect("name is some");
            slot.stable_id = stable_id.clone();
            slot.started = true;
            events.push(GenerateStreamEvent::ToolCallStart {
                id: stable_id,
                name,
            });
        }

        // 参数增量:已 started 才 emit Delta,否则仅累积(防止 args 早于 name 到达)。
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

    /// 流结束时为每个已开始的 tool call emit End 事件。
    fn drain_ends(&self) -> Vec<GenerateStreamEvent> {
        self.by_index
            .iter()
            .filter_map(|(_, state)| {
                if state.started {
                    Some(GenerateStreamEvent::ToolCallEnd {
                        id: state.stable_id.clone(),
                    })
                } else {
                    None
                }
            })
            .collect()
    }

    /// 汇总成 ToolCallBlock(按 index 顺序)。
    fn finish(self) -> Vec<ToolCallBlock> {
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
                ToolCallBlock {
                    id,
                    name,
                    input,
                    state: ToolCallState::Submitted,
                }
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use anvil_core::ai::{GenerateRequest, Message, Role};

    #[test]
    fn builds_text_chat_completion_body() {
        let provider = OpenAiCompatibleChatProvider::qwen_dashscope("test-key");
        let req = GenerateRequest {
            model: "qwen-plus".to_string(),
            messages: vec![Message::text(Role::User, "hello")],
            temperature: None,
            max_tokens: Some(64),
            stream: false,
            thinking: None,
            tools: Vec::new(),
        };

        let body = provider.chat_completions_request_body(&req).unwrap();

        assert_eq!(body["model"], "qwen-plus");
        assert_eq!(body["messages"][0]["role"], "user");
        assert_eq!(body["messages"][0]["content"], "hello");
        assert_eq!(body["max_tokens"], 64);
    }

    #[test]
    fn extra_body_overrides_or_extends_request() {
        let mut extra = Map::new();
        extra.insert("reasoning_effort".to_string(), json!("high"));
        let provider = OpenAiCompatibleChatProvider::deepseek("test-key").with_extra_body(extra);
        let req = GenerateRequest::text("deepseek-chat", "hello");

        let body = provider.chat_completions_request_body(&req).unwrap();

        assert_eq!(body["reasoning_effort"], "high");
    }

    #[test]
    fn rejects_provider_specific_thinking_mode() {
        let provider = OpenAiCompatibleChatProvider::kimi("test-key");
        let req = GenerateRequest::text("kimi-k2.6", "hello")
            .with_thinking(anvil_core::ai::ThinkingConfig::enabled());

        assert!(provider.chat_completions_request_body(&req).is_err());
    }

    #[test]
    fn parses_chat_completion_text() {
        let raw = json!({
            "choices": [{
                "message": { "content": "hello" }
            }],
            "usage": {
                "prompt_tokens": 2,
                "completion_tokens": 3,
                "total_tokens": 5
            }
        });

        let response = OpenAiCompatibleChatProvider::parse_generate_response(raw);

        assert_eq!(response.text, "hello");
        assert_eq!(response.usage.unwrap().output_tokens, Some(3));
    }

    #[test]
    fn parses_reasoning_content() {
        let raw = json!({
            "choices": [{
                "message": {
                    "reasoning_content": "thinking",
                    "content": "answer"
                }
            }]
        });

        let response = OpenAiCompatibleChatProvider::parse_generate_response(raw);

        assert_eq!(response.reasoning_text.as_deref(), Some("thinking"));
        assert_eq!(response.text, "answer");
    }

    #[test]
    fn extracts_stream_text_and_reasoning_deltas() {
        let raw = json!({
            "choices": [{
                "delta": {
                    "reasoning_content": "think",
                    "content": "answer"
                }
            }]
        });

        assert_eq!(chat_delta(&raw).as_deref(), Some("answer"));
        assert_eq!(chat_reasoning_delta(&raw).as_deref(), Some("think"));
    }

    #[test]
    fn preserves_assistant_thinking_blocks_in_history() {
        let provider = OpenAiCompatibleChatProvider::deepseek("test-key");
        let req = GenerateRequest {
            model: "deepseek-v4-pro".to_string(),
            messages: vec![Message::assistant_with_thinking("answer", "reasoning")],
            temperature: None,
            max_tokens: None,
            stream: false,
            thinking: None,
            tools: Vec::new(),
        };

        let body = provider.chat_completions_request_body(&req).unwrap();

        assert_eq!(body["messages"][0]["reasoning_content"], "reasoning");
        assert_eq!(body["messages"][0]["content"][0]["text"], "answer");
    }

    #[test]
    fn parses_tool_calls_from_completion() {
        let raw = json!({
            "choices": [{
                "message": {
                    "content": null,
                    "tool_calls": [{
                        "id": "call_abc",
                        "type": "function",
                        "function": {
                            "name": "read",
                            "arguments": "{\"path\":\"Cargo.toml\"}"
                        }
                    }]
                }
            }]
        });

        let response = OpenAiCompatibleChatProvider::parse_generate_response(raw);

        assert_eq!(response.tool_calls.len(), 1);
        let tc = &response.tool_calls[0];
        assert_eq!(tc.id, "call_abc");
        assert_eq!(tc.name, "read");
        assert_eq!(tc.input, "{\"path\":\"Cargo.toml\"}");
    }

    #[test]
    fn accumulates_streaming_tool_calls() {
        let mut acc = ToolCallAccumulator::new();

        // chunk 1:index 0 + id + name + 部分 args
        let mut events = acc.on_delta(&json!({
            "index": 0,
            "id": "call_1",
            "function": { "name": "read", "arguments": "{\"path\":" }
        }));
        // chunk 2:更多 args(同一 index)
        events.extend(acc.on_delta(&json!({
            "index": 0,
            "function": { "arguments": "\"Cargo.toml\"}" }
        })));

        let kinds: Vec<&str> = events
            .iter()
            .map(|ev| match ev {
                GenerateStreamEvent::ToolCallStart { .. } => "start",
                GenerateStreamEvent::ToolCallDelta { .. } => "delta",
                GenerateStreamEvent::ToolCallEnd { .. } => "end",
                _ => "other",
            })
            .collect();
        assert_eq!(kinds, vec!["start", "delta", "delta"]);

        let ends = acc.drain_ends();
        assert_eq!(ends.len(), 1);

        let tool_calls = acc.finish();
        assert_eq!(tool_calls.len(), 1);
        assert_eq!(tool_calls[0].id, "call_1");
        assert_eq!(tool_calls[0].name, "read");
        assert_eq!(tool_calls[0].input, "{\"path\":\"Cargo.toml\"}");
    }

    #[test]
    fn includes_tools_in_request_body() {
        let provider = OpenAiCompatibleChatProvider::deepseek("test-key");
        let req = GenerateRequest {
            model: "deepseek-chat".to_string(),
            messages: vec![Message::text(Role::User, "list files")],
            temperature: None,
            max_tokens: None,
            stream: false,
            thinking: None,
            tools: vec![anvil_core::ai::ToolDefinition {
                name: "list".to_string(),
                description: "list dir".to_string(),
                parameters: json!({"type": "object", "properties": {}}),
            }],
        };

        let body = provider.chat_completions_request_body(&req).unwrap();

        assert_eq!(body["tools"][0]["type"], "function");
        assert_eq!(body["tools"][0]["function"]["name"], "list");
        assert_eq!(body["tool_choice"], "auto");
    }
}
