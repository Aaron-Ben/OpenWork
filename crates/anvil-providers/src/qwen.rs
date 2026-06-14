use anvil_core::ai::{
    EmbeddingProvider, EmbeddingRequest, EmbeddingResponse, GenerateRequest, GenerateResponse,
    GenerateStreamCallback, LlmProvider, ProviderError,
};
use async_trait::async_trait;
use reqwest::header::{AUTHORIZATION, CONTENT_TYPE, HeaderMap, HeaderValue};
use serde_json::{Map, Value};

use crate::{
    HttpProviderConfig, OpenAiCompatibleChatProvider, OpenAiProvider,
    error::{map_error_response, map_reqwest_error},
};

const DASHSCOPE_COMPATIBLE_BASE_URL: &str = "https://dashscope.aliyuncs.com/compatible-mode/v1";

#[derive(Debug, Clone)]
pub struct QwenProvider {
    chat: OpenAiCompatibleChatProvider,
    embedding_client: reqwest::Client,
    config: HttpProviderConfig,
}

impl QwenProvider {
    pub fn new(config: HttpProviderConfig) -> Self {
        Self {
            chat: OpenAiCompatibleChatProvider::new(config.clone()),
            embedding_client: reqwest::Client::new(),
            config,
        }
    }

    pub fn from_api_key(api_key: impl Into<String>) -> Self {
        Self::new(HttpProviderConfig::new(
            DASHSCOPE_COMPATIBLE_BASE_URL,
            api_key,
        ))
    }

    pub fn with_extra_body(mut self, extra_body: Map<String, Value>) -> Self {
        self.chat = self.chat.with_extra_body(extra_body);
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

    #[cfg(test)]
    pub(crate) fn chat_request_body(&self, req: &GenerateRequest) -> Result<Value, ProviderError> {
        self.chat.chat_completions_request_body(req)
    }

    pub(crate) fn embedding_request_body(req: &EmbeddingRequest) -> Value {
        OpenAiProvider::embeddings_request_body(req)
    }

    pub(crate) fn parse_embedding_response(raw: Value) -> Result<EmbeddingResponse, ProviderError> {
        OpenAiProvider::parse_embedding_response(raw)
    }
}

#[async_trait]
impl LlmProvider for QwenProvider {
    async fn generate(&self, req: GenerateRequest) -> Result<GenerateResponse, ProviderError> {
        self.chat.generate(req).await
    }

    async fn stream_generate(
        &self,
        req: GenerateRequest,
        on_event: GenerateStreamCallback,
    ) -> Result<GenerateResponse, ProviderError> {
        self.chat.stream_generate(req, on_event).await
    }
}

#[async_trait]
impl EmbeddingProvider for QwenProvider {
    async fn embed(&self, req: EmbeddingRequest) -> Result<EmbeddingResponse, ProviderError> {
        let body = Self::embedding_request_body(&req);
        let response = self
            .embedding_client
            .post(self.config.endpoint("/embeddings"))
            .headers(self.headers()?)
            .json(&body)
            .send()
            .await
            .map_err(map_reqwest_error)?;

        if !response.status().is_success() {
            return Err(map_error_response(response).await);
        }

        let raw = response.json::<Value>().await.map_err(map_reqwest_error)?;
        Self::parse_embedding_response(raw)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use anvil_core::ai::{ContentBlock, Message, Role};
    use serde_json::json;

    #[test]
    fn builds_qwen_chat_body() {
        let provider = QwenProvider::from_api_key("test-key");
        let req = GenerateRequest::text("qwen-plus", "hello");

        let body = provider.chat_request_body(&req).unwrap();

        assert_eq!(body["model"], "qwen-plus");
        assert_eq!(body["messages"][0]["content"], "hello");
    }

    #[test]
    fn builds_qwen_multimodal_chat_body() {
        let provider = QwenProvider::from_api_key("test-key");
        let req = GenerateRequest {
            model: "qwen-vl-plus".to_string(),
            messages: vec![Message {
                role: Role::User,
                content: vec![
                    ContentBlock::text("describe this image"),
                    ContentBlock::image_url("https://example.com/image.png", "image/png"),
                ],
            }],
            temperature: None,
            max_tokens: None,
            stream: false,
            thinking: None,
        };

        let body = provider.chat_request_body(&req).unwrap();

        assert_eq!(body["messages"][0]["content"][0]["type"], "text");
        assert_eq!(body["messages"][0]["content"][1]["type"], "image_url");
    }

    #[test]
    fn builds_qwen_embedding_body() {
        let req = EmbeddingRequest {
            model: "text-embedding-v4".to_string(),
            input: vec!["hello".to_string()],
            dimensions: Some(1024),
        };

        let body = QwenProvider::embedding_request_body(&req);

        assert_eq!(body["model"], "text-embedding-v4");
        assert_eq!(body["input"][0], "hello");
        assert_eq!(body["dimensions"], 1024);
    }

    #[test]
    fn parses_qwen_embedding_response() {
        let raw = json!({
            "data": [{ "embedding": [0.1, 0.2, 0.3] }],
            "usage": { "prompt_tokens": 4, "total_tokens": 4 }
        });

        let response = QwenProvider::parse_embedding_response(raw).unwrap();

        assert_eq!(response.vectors, vec![vec![0.1, 0.2, 0.3]]);
        assert_eq!(response.usage.unwrap().total_tokens, Some(4));
    }
}
