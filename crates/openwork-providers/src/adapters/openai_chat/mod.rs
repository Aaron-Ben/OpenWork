pub mod dialect;
pub(crate) mod error;
pub(crate) mod request;
pub(crate) mod response;
pub(crate) mod stream;

use async_trait::async_trait;
use openwork_protocol::model::{
    ModelCallOptions, ModelError, ModelPort, ModelRequest, ModelResponse, ModelStream,
};
use openwork_protocol::provider::OpenAiChatDialect;
use reqwest::header::{AUTHORIZATION, CONTENT_TYPE, HeaderMap, HeaderValue};
use serde_json::{Map, Value};

use self::{response::ResponseAccumulator, stream::ToolStream};
use crate::{
    config::{HttpProviderConfig, HttpTransport},
    error::{
        ErrorDialect, decode_stream_json, map_error_response_for, map_reqwest_error,
        map_stream_error_event_for, request_id_from_headers,
    },
    sse::consume_sse_response,
    stream::{EventCallback, model_stream_from_callback},
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

    async fn stream_generate(
        &self,
        req: ModelRequest,
        mut on_event: EventCallback,
    ) -> Result<ModelResponse, ModelError> {
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

        let mut accumulator = ResponseAccumulator::new();
        let mut tools = ToolStream::new();
        consume_sse_response(response, |data| {
            if data == "[DONE]" {
                return Ok(true);
            }
            let event = decode_stream_json(data)?;
            if let Some(error) = map_stream_error_event_for(&event, self.error_dialect()) {
                return Err(error);
            }
            let (response_events, terminal) = accumulator.observe(&event);
            for response_event in response_events {
                on_event(response_event);
            }
            if let Some(tool_call_deltas) = event
                .pointer("/choices/0/delta/tool_calls")
                .and_then(Value::as_array)
            {
                for tool_call in tool_call_deltas {
                    for tool_event in tools.append_openai_chat_delta(tool_call) {
                        on_event(tool_event);
                    }
                }
            }
            Ok(terminal)
        })
        .await?;

        for event in tools.drain_ends() {
            on_event(event);
        }
        let tool_calls = tools.finish().map_err(ModelError::invalid_request)?;
        Ok(accumulator.finish(provider_request_id, req.model, tool_calls))
    }
}

#[async_trait]
impl ModelPort for OpenAiCompatibleChatProvider {
    async fn invoke(
        &self,
        request: ModelRequest,
        _options: ModelCallOptions,
    ) -> Result<ModelStream, ModelError> {
        let provider = self.clone();
        Ok(model_stream_from_callback(move |callback| async move {
            provider.stream_generate(request, callback).await
        }))
    }
}
