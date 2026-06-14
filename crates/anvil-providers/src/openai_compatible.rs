use anvil_core::ai::{
    GenerateRequest, GenerateResponse, GenerateStreamCallback, GenerateStreamEvent, LlmProvider,
    ProviderError,
};
use async_trait::async_trait;
use reqwest::header::{AUTHORIZATION, CONTENT_TYPE, HeaderMap, HeaderValue};
use serde_json::{Map, Value, json};

use crate::{
    config::HttpProviderConfig,
    error::{map_error_response, map_reqwest_error},
    serde_helpers::{
        openai_chat_message, reasoning_text_from_chat_completion,
        response_text_from_chat_completion, usage_from_openai,
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

        for (key, value) in &self.extra_body {
            body.insert(key.clone(), value.clone());
        }

        Ok(Value::Object(body))
    }

    pub(crate) fn parse_generate_response(raw: Value) -> GenerateResponse {
        let text = response_text_from_chat_completion(&raw);
        let reasoning_text = reasoning_text_from_chat_completion(&raw);
        let usage = usage_from_openai(&raw);
        GenerateResponse {
            text,
            reasoning_text,
            usage,
            raw,
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
        let raw_events = consume_sse_response(response, |event| {
            if let Some(delta) = chat_delta(event) {
                text.push_str(&delta);
                on_event(GenerateStreamEvent::TextDelta { delta });
            }
            if let Some(delta) = chat_reasoning_delta(event) {
                reasoning_text.push_str(&delta);
                on_event(GenerateStreamEvent::ReasoningDelta { delta });
            }
            Ok(())
        })
        .await?;

        Ok(GenerateResponse {
            text,
            reasoning_text: if reasoning_text.is_empty() {
                None
            } else {
                Some(reasoning_text)
            },
            usage: None,
            raw: Value::Array(raw_events),
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
        };

        let body = provider.chat_completions_request_body(&req).unwrap();

        assert_eq!(body["messages"][0]["reasoning_content"], "reasoning");
        assert_eq!(body["messages"][0]["content"][0]["text"], "answer");
    }
}
