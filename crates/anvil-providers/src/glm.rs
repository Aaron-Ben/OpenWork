use anvil_core::ai::{
    GenerateRequest, GenerateResponse, GenerateStreamCallback, LlmProvider, ProviderError,
};
use async_trait::async_trait;
use serde_json::{Map, Value};

use crate::{HttpProviderConfig, OpenAiCompatibleChatProvider};

const GLM_BASE_URL: &str = "https://open.bigmodel.cn/api/paas/v4";

#[derive(Debug, Clone)]
pub struct GlmProvider {
    inner: OpenAiCompatibleChatProvider,
}

impl GlmProvider {
    pub fn new(config: HttpProviderConfig) -> Self {
        Self {
            inner: OpenAiCompatibleChatProvider::new(config),
        }
    }

    pub fn from_api_key(api_key: impl Into<String>) -> Self {
        Self::new(HttpProviderConfig::new(GLM_BASE_URL, api_key))
    }

    pub fn with_extra_body(mut self, extra_body: Map<String, Value>) -> Self {
        self.inner = self.inner.with_extra_body(extra_body);
        self
    }
}

#[async_trait]
impl LlmProvider for GlmProvider {
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
    fn builds_default_glm_provider() {
        let provider = GlmProvider::from_api_key("test-key");
        let req = GenerateRequest::text("glm-4.6", "hello");

        let body = provider.inner.chat_completions_request_body(&req).unwrap();

        assert_eq!(body["model"], "glm-4.6");
        assert_eq!(body["messages"][0]["content"], "hello");
    }

    #[test]
    fn accepts_glm_specific_extra_body() {
        let mut extra = Map::new();
        extra.insert("request_id".to_string(), json!("req-test"));
        let provider = GlmProvider::from_api_key("test-key").with_extra_body(extra);
        let req = GenerateRequest::text("glm-4.6", "hello");

        let body = provider.inner.chat_completions_request_body(&req).unwrap();

        assert_eq!(body["request_id"], "req-test");
    }
}
