pub(crate) mod request;
pub(crate) mod response;
pub(crate) mod stream;

use crate::model::{ModelError, ModelRequest, ModelStream};
use crate::provider::OpenAiChatDialect;
use reqwest::header::{AUTHORIZATION, CONTENT_TYPE, HeaderMap, HeaderValue};
use serde_json::{Map, Value};

use crate::{
    HttpProviderConfig, HttpTransport,
    error::{ErrorDialect, map_error_response_for, map_reqwest_error, request_id_from_headers},
};

fn error_dialect(dialect: OpenAiChatDialect) -> ErrorDialect {
    match dialect {
        OpenAiChatDialect::Deepseek => ErrorDialect::DeepSeek,
        OpenAiChatDialect::Kimi => ErrorDialect::Kimi,
        OpenAiChatDialect::Qwen => ErrorDialect::Qwen,
        OpenAiChatDialect::Glm => ErrorDialect::Glm,
    }
}

fn headers(config: &HttpProviderConfig) -> Result<HeaderMap, ModelError> {
    let mut headers = HeaderMap::new();
    let auth = HeaderValue::from_str(&format!("Bearer {}", config.api_key()))
        .map_err(|error| ModelError::invalid_request(error.to_string()))?;
    headers.insert(AUTHORIZATION, auth);
    headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    Ok(headers)
}

pub(crate) async fn start_stream(
    config: &HttpProviderConfig,
    transport: &HttpTransport,
    dialect: OpenAiChatDialect,
    extra_body: &Map<String, Value>,
    req: ModelRequest,
) -> Result<ModelStream, ModelError> {
    let body = request::encode_request(&req, true, dialect, extra_body)?;
    let response = transport
        .post_json(
            config.endpoint("/chat/completions"),
            headers(config)?,
            &body,
        )
        .await
        .map_err(map_reqwest_error)?;

    let error_dialect = error_dialect(dialect);
    if !response.status().is_success() {
        return Err(map_error_response_for(response, error_dialect).await);
    }
    let provider_request_id = request_id_from_headers(response.headers());
    Ok(stream::response_stream(
        response,
        provider_request_id,
        req.model,
        error_dialect,
    ))
}
