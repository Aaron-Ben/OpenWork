pub(crate) mod error;
mod request;
mod response;
mod stream;

use async_trait::async_trait;
use openwork_protocol::model::{
    ModelCallOptions, ModelError, ModelPort, ModelRequest, ModelResponse, ModelStream,
};
use reqwest::header::{CONTENT_TYPE, HeaderMap, HeaderName, HeaderValue};

use self::{
    response::ResponseAccumulator,
    stream::{AnthropicThinkingStream, AnthropicToolStream},
};
use crate::{
    config::HttpProviderConfig,
    error::{
        ErrorDialect, decode_stream_json, map_error_response_for, map_reqwest_error,
        map_stream_error_event_for, request_id_from_headers,
    },
    sse::consume_sse_response,
    stream::{EventCallback, model_stream_from_callback},
};

const ANTHROPIC_VERSION: &str = "2023-06-01";

#[derive(Debug, Clone)]
pub struct AnthropicProvider {
    client: reqwest::Client,
    config: HttpProviderConfig,
    anthropic_version: String,
}

impl AnthropicProvider {
    pub fn new(config: HttpProviderConfig) -> Self {
        Self {
            client: reqwest::Client::new(),
            config,
            anthropic_version: ANTHROPIC_VERSION.to_string(),
        }
    }

    pub fn from_api_key(api_key: impl Into<String>) -> Self {
        Self::new(HttpProviderConfig::new(
            "https://api.anthropic.com",
            api_key,
        ))
    }

    fn headers(&self) -> Result<HeaderMap, ModelError> {
        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        headers.insert(
            HeaderName::from_static("anthropic-version"),
            HeaderValue::from_str(&self.anthropic_version)
                .map_err(|error| ModelError::invalid_request(error.to_string()))?,
        );
        headers.insert(
            HeaderName::from_static("x-api-key"),
            HeaderValue::from_str(&self.config.api_key)
                .map_err(|error| ModelError::invalid_request(error.to_string()))?,
        );
        Ok(headers)
    }

    async fn stream_generate(
        &self,
        req: ModelRequest,
        mut on_event: EventCallback,
    ) -> Result<ModelResponse, ModelError> {
        let body = request::encode_request(&req, true)?;
        let response = self
            .client
            .post(self.config.endpoint("/v1/messages"))
            .headers(self.headers()?)
            .json(&body)
            .send()
            .await
            .map_err(map_reqwest_error)?;

        if !response.status().is_success() {
            return Err(map_error_response_for(response, ErrorDialect::Anthropic).await);
        }
        let provider_request_id = request_id_from_headers(response.headers());

        let mut accumulator = ResponseAccumulator::default();
        let mut tools = AnthropicToolStream::default();
        let mut thinking_blocks = AnthropicThinkingStream::default();
        consume_sse_response(response, |data| {
            let event = decode_stream_json(data)?;
            if let Some(error) = map_stream_error_event_for(&event, ErrorDialect::Anthropic) {
                return Err(error);
            }
            let (response_events, terminal) = accumulator.observe(&event);
            for response_event in response_events {
                on_event(response_event);
            }
            for tool_event in tools.observe(&event) {
                on_event(tool_event);
            }
            thinking_blocks.observe(&event);
            Ok(terminal)
        })
        .await?;

        let (tool_calls, tool_end_events) = tools.finish().map_err(ModelError::protocol)?;
        for event in tool_end_events {
            on_event(event);
        }

        Ok(accumulator.finish(
            provider_request_id,
            req.model,
            tool_calls,
            thinking_blocks.finish(),
        ))
    }
}

#[async_trait]
impl ModelPort for AnthropicProvider {
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
