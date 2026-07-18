mod request;
mod response;
mod stream;

use crate::model::{ModelError, ModelRequest, ModelStream};
use reqwest::header::{AUTHORIZATION, CONTENT_TYPE, HeaderMap, HeaderValue};

use crate::{
    HttpProviderConfig, HttpTransport,
    error::{ErrorDialect, map_error_response_for, map_reqwest_error, request_id_from_headers},
};

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
    req: ModelRequest,
) -> Result<ModelStream, ModelError> {
    let body = request::encode_request(&req, true)?;
    let response = transport
        .post_json(config.endpoint("/v1/responses"), headers(config)?, &body)
        .await
        .map_err(map_reqwest_error)?;

    if !response.status().is_success() {
        return Err(map_error_response_for(response, ErrorDialect::OpenAi).await);
    }
    let provider_request_id = request_id_from_headers(response.headers());
    Ok(stream::response_stream(
        response,
        provider_request_id,
        req.model,
    ))
}
