use async_trait::async_trait;
use openwork_protocol::ai::{
    GenerateRequest, GenerateResponse, GenerateStreamCallback, GenerateStreamEvent, LlmProvider,
    ProviderError, ThinkingMode,
};
use reqwest::header::{AUTHORIZATION, CONTENT_TYPE, HeaderMap, HeaderValue};
use serde_json::{Map, Value, json};

use crate::{
    HttpProviderConfig, OpenAiCompatibleChatProvider,
    error::{map_error_response, map_reqwest_error},
    openai_compatible::{chat_delta, chat_reasoning_delta},
    sse::consume_sse_response,
};

const KIMI_BASE_URL: &str = "https://api.moonshot.cn";

#[derive(Debug, Clone)]
pub struct KimiProvider {
    client: reqwest::Client,
    config: HttpProviderConfig,
    base_chat: OpenAiCompatibleChatProvider,
    extra_body: Map<String, Value>,
}

impl KimiProvider {
    pub fn new(config: HttpProviderConfig) -> Self {
        Self {
            client: reqwest::Client::new(),
            base_chat: OpenAiCompatibleChatProvider::new(config.clone()),
            config,
            extra_body: Map::new(),
        }
    }

    pub fn from_api_key(api_key: impl Into<String>) -> Self {
        Self::new(HttpProviderConfig::new(KIMI_BASE_URL, api_key))
    }

    pub fn with_extra_body(mut self, extra_body: Map<String, Value>) -> Self {
        self.extra_body = extra_body;
        self
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
        if !req.tools.is_empty() {
            return Err(ProviderError::InvalidRequest {
                message:
                    "tool calling is not supported for the Kimi provider yet; use openai_compatible"
                        .to_string(),
            });
        }
        let mut base_req = req.clone();
        base_req.thinking = None;
        let mut body = self.base_chat.chat_completions_request_body(&base_req)?;
        let Some(body_object) = body.as_object_mut() else {
            return Err(ProviderError::Serialization {
                message: "OpenAI-compatible chat body was not an object".to_string(),
            });
        };

        if let Some(thinking) = req.thinking {
            body_object.insert(
                "thinking".to_string(),
                json!({ "type": kimi_thinking_type(thinking.mode) }),
            );
        }

        for (key, value) in &self.extra_body {
            body_object.insert(key.clone(), value.clone());
        }

        Ok(body)
    }

    pub(crate) fn parse_generate_response(raw: Value) -> GenerateResponse {
        OpenAiCompatibleChatProvider::parse_generate_response(raw)
    }
}

#[async_trait]
impl LlmProvider for KimiProvider {
    async fn generate(&self, req: GenerateRequest) -> Result<GenerateResponse, ProviderError> {
        if req.stream {
            return Err(ProviderError::InvalidRequest {
                message: "streaming is not implemented for Kimi provider yet".to_string(),
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
            tool_calls: Vec::new(),
        })
    }
}

fn kimi_thinking_type(mode: ThinkingMode) -> &'static str {
    match mode {
        ThinkingMode::Enabled => "enabled",
        ThinkingMode::Disabled => "disabled",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use openwork_protocol::ai::{GenerateRequest, ThinkingConfig};

    #[test]
    fn builds_kimi_request_with_thinking_enabled() {
        let provider = KimiProvider::from_api_key("test-key");
        let req =
            GenerateRequest::text("kimi-k2.6", "solve it").with_thinking(ThinkingConfig::enabled());

        let body = provider.chat_completions_request_body(&req).unwrap();

        assert_eq!(body["model"], "kimi-k2.6");
        assert_eq!(body["thinking"]["type"], "enabled");
    }

    #[test]
    fn builds_kimi_request_with_thinking_disabled() {
        let provider = KimiProvider::from_api_key("test-key");
        let req = GenerateRequest::text("kimi-k2.6", "answer directly")
            .with_thinking(ThinkingConfig::disabled());

        let body = provider.chat_completions_request_body(&req).unwrap();

        assert_eq!(body["thinking"]["type"], "disabled");
    }

    #[test]
    fn parses_kimi_reasoning_content() {
        let raw = json!({
            "choices": [{
                "message": {
                    "reasoning_content": "first think",
                    "content": "final answer"
                }
            }]
        });

        let response = KimiProvider::parse_generate_response(raw);

        assert_eq!(response.reasoning_text.as_deref(), Some("first think"));
        assert_eq!(response.text, "final answer");
    }
}
