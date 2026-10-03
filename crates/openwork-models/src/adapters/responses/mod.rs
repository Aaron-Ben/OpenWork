//! Responses 线协议：`POST {base_url}/responses`，SSE 流式响应。

mod request;
mod response;
mod stream;

use crate::model::{ModelError, ModelRequest, ModelStream};
use reqwest::header::{AUTHORIZATION, CONTENT_TYPE, HeaderMap, HeaderName, HeaderValue};

use crate::{
    HttpProviderConfig, HttpTransport,
    error::{map_error_response, map_reqwest_error, request_id_from_headers},
};

fn headers(config: &HttpProviderConfig) -> Result<HeaderMap, ModelError> {
    let mut headers = HeaderMap::new();
    for (name, value) in config.http_headers() {
        let name = HeaderName::from_bytes(name.as_bytes())
            .map_err(|error| ModelError::invalid_request(format!("http header {name}: {error}")))?;
        let value = HeaderValue::from_str(value)
            .map_err(|error| ModelError::invalid_request(format!("http header {name}: {error}")))?;
        headers.insert(name, value);
    }
    let auth = HeaderValue::from_str(&format!("Bearer {}", config.api_key()))
        .map_err(|error| ModelError::invalid_request(error.to_string()))?;
    headers.insert(AUTHORIZATION, auth);
    headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    Ok(headers)
}

pub(crate) async fn start_stream(
    config: &HttpProviderConfig,
    transport: &HttpTransport,
    req: ModelRequest,
) -> Result<ModelStream, ModelError> {
    let body = request::encode_request(&req)?;
    let response = transport
        .post_json(config.endpoint("/responses"), headers(config)?, &body)
        .await
        .map_err(map_reqwest_error)?;

    if !response.status().is_success() {
        return Err(map_error_response(response).await);
    }
    let provider_request_id = request_id_from_headers(response.headers());
    Ok(stream::response_stream(
        response,
        provider_request_id,
        req.model,
    ))
}
