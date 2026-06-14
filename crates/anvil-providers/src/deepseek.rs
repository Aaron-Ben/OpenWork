use anvil_core::ai::{
    GenerateRequest, GenerateResponse, GenerateStreamCallback, LlmProvider, ProviderError,
};
use async_trait::async_trait;
use serde_json::{Map, Value};

use crate::{HttpProviderConfig, OpenAiCompatibleChatProvider};

#[derive(Debug, Clone)]
pub struct DeepSeekProvider {
    inner: OpenAiCompatibleChatProvider,
}

impl DeepSeekProvider {
    pub fn new(config: HttpProviderConfig) -> Self {
        Self {
            inner: OpenAiCompatibleChatProvider::new(config),
        }
    }

    pub fn from_api_key(api_key: impl Into<String>) -> Self {
        Self {
            inner: OpenAiCompatibleChatProvider::deepseek(api_key),
        }
    }

    pub fn with_extra_body(mut self, extra_body: Map<String, Value>) -> Self {
        self.inner = self.inner.with_extra_body(extra_body);
        self
    }
}

#[async_trait]
impl LlmProvider for DeepSeekProvider {
    async fn generate(&self, req: GenerateRequest) -> Result<GenerateResponse, ProviderError> {
        self.inner.generate(req).await
    }

    async fn stream_generate(
        &self,
        req: GenerateRequest,
        on_event: GenerateStreamCallback,
    ) -> Result<GenerateResponse, ProviderError> {
        self.inner.stream_generate(req, on_event).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use anvil_core::ai::GenerateRequest;
    use serde_json::json;

    #[test]
    fn builds_default_deepseek_provider() {
        let provider = DeepSeekProvider::from_api_key("test-key");
        let req = GenerateRequest::text("deepseek-chat", "hello");

        let body = provider.inner.chat_completions_request_body(&req).unwrap();

        assert_eq!(body["model"], "deepseek-chat");
        assert_eq!(body["messages"][0]["content"], "hello");
    }

    #[test]
    fn accepts_deepseek_specific_extra_body() {
        let mut extra = Map::new();
        extra.insert("reasoning_effort".to_string(), json!("high"));
        let provider = DeepSeekProvider::from_api_key("test-key").with_extra_body(extra);
        let req = GenerateRequest::text("deepseek-reasoner", "think");

        let body = provider.inner.chat_completions_request_body(&req).unwrap();

        assert_eq!(body["reasoning_effort"], "high");
    }
}
