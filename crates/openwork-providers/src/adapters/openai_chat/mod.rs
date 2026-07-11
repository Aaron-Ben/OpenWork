pub mod dialect;
pub(crate) mod error;
pub(crate) mod request;
pub(crate) mod response;
pub(crate) mod stream;

use async_trait::async_trait;
#[cfg(test)]
use openwork_protocol::model::ModelResponse;
use openwork_protocol::model::{
    ModelCallOptions, ModelError, ModelPort, ModelRequest, ModelStream,
};
use openwork_protocol::provider::OpenAiChatDialect;
use reqwest::header::{AUTHORIZATION, CONTENT_TYPE, HeaderMap, HeaderValue};
use serde_json::{Map, Value};

use crate::{
    config::{HttpProviderConfig, HttpTransport},
    error::{ErrorDialect, map_error_response_for, map_reqwest_error, request_id_from_headers},
};

#[derive(Debug, Clone)]
pub struct OpenAiCompatibleChatProvider {
    transport: HttpTransport,
    config: HttpProviderConfig,
    extra_body: Map<String, Value>,
    dialect: OpenAiChatDialect,
}

impl OpenAiCompatibleChatProvider {
    pub fn new(config: HttpProviderConfig, transport: HttpTransport) -> Self {
        Self {
            transport,
            config,
            extra_body: Map::new(),
            dialect: OpenAiChatDialect::Standard,
        }
    }

    pub fn with_extra_body(mut self, extra_body: Map<String, Value>) -> Self {
        self.extra_body = extra_body;
        self
    }

    pub(crate) fn with_dialect(mut self, dialect: OpenAiChatDialect) -> Self {
        self.dialect = dialect;
        self
    }

    pub fn kimi(api_key: impl Into<String>, transport: HttpTransport) -> Self {
        Self::new(
            HttpProviderConfig::new("https://api.moonshot.cn/v1", api_key),
            transport,
        )
        .with_dialect(OpenAiChatDialect::Kimi)
    }

    pub fn deepseek(api_key: impl Into<String>, transport: HttpTransport) -> Self {
        Self::new(
            HttpProviderConfig::new("https://api.deepseek.com", api_key),
            transport,
        )
        .with_dialect(OpenAiChatDialect::Deepseek)
    }

    pub fn qwen_dashscope(api_key: impl Into<String>, transport: HttpTransport) -> Self {
        Self::new(
            HttpProviderConfig::new("https://dashscope.aliyuncs.com/compatible-mode/v1", api_key),
            transport,
        )
        .with_dialect(OpenAiChatDialect::Qwen)
    }

    fn error_dialect(&self) -> ErrorDialect {
        match self.dialect {
            OpenAiChatDialect::Standard => ErrorDialect::StandardOpenAiChat,
            OpenAiChatDialect::Deepseek => ErrorDialect::DeepSeek,
            OpenAiChatDialect::Kimi => ErrorDialect::Kimi,
            OpenAiChatDialect::Qwen => ErrorDialect::Qwen,
            OpenAiChatDialect::Glm => ErrorDialect::Glm,
        }
    }

    fn headers(&self) -> Result<HeaderMap, ModelError> {
        let mut headers = HeaderMap::new();
        let auth = HeaderValue::from_str(&format!("Bearer {}", self.config.api_key))
            .map_err(|error| ModelError::invalid_request(error.to_string()))?;
        headers.insert(AUTHORIZATION, auth);
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        Ok(headers)
    }

    #[cfg(test)]
    pub(crate) fn chat_completions_request_body(
        &self,
        req: &ModelRequest,
        stream: bool,
    ) -> Result<Value, ModelError> {
        request::encode_request(req, stream, self.dialect, &self.extra_body)
    }

    #[cfg(test)]
    pub(crate) fn parse_generate_response(raw: Value) -> ModelResponse {
        response::parse_buffered(raw)
    }

    async fn start_stream(&self, req: ModelRequest) -> Result<ModelStream, ModelError> {
        let body = request::encode_request(&req, true, self.dialect, &self.extra_body)?;
        let response = self
            .transport
            .post_json(
                self.config.endpoint("/chat/completions"),
                self.headers()?,
                &body,
            )
            .await
            .map_err(map_reqwest_error)?;

        if !response.status().is_success() {
            return Err(map_error_response_for(response, self.error_dialect()).await);
        }
        let provider_request_id = request_id_from_headers(response.headers());
        Ok(stream::response_stream(
            response,
            provider_request_id,
            req.model,
            self.error_dialect(),
        ))
    }
}

#[async_trait]
impl ModelPort for OpenAiCompatibleChatProvider {
    async fn invoke(
        &self,
        request: ModelRequest,
        _options: ModelCallOptions,
    ) -> Result<ModelStream, ModelError> {
        self.start_stream(request).await
    }
}
