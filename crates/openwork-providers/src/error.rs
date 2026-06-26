use openwork_protocol::ai::ProviderError;
use reqwest::{Response, StatusCode};

pub fn map_reqwest_error(error: reqwest::Error) -> ProviderError {
    if error.is_timeout() {
        return ProviderError::Timeout;
    }

    if error.is_decode() {
        return ProviderError::Serialization {
            message: error.to_string(),
        };
    }

    ProviderError::Network {
        message: error.to_string(),
    }
}

pub async fn map_error_response(response: Response) -> ProviderError {
    let status = response.status();
    let retry_after_ms = response
        .headers()
        .get(reqwest::header::RETRY_AFTER)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u64>().ok())
        .map(|seconds| seconds * 1_000);

    match status {
        StatusCode::UNAUTHORIZED => ProviderError::Authentication,
        StatusCode::FORBIDDEN => ProviderError::PermissionDenied,
        StatusCode::TOO_MANY_REQUESTS => ProviderError::RateLimited { retry_after_ms },
        StatusCode::BAD_REQUEST | StatusCode::UNPROCESSABLE_ENTITY => {
            let message = response
                .text()
                .await
                .unwrap_or_else(|_| "provider rejected request".to_string());
            ProviderError::InvalidRequest { message }
        }
        status if status.is_server_error() => ProviderError::ProviderServerError {
            status: status.as_u16(),
        },
        status => {
            let message = response
                .text()
                .await
                .unwrap_or_else(|_| "unexpected provider error".to_string());
            ProviderError::InvalidRequest {
                message: format!("status {}: {}", status.as_u16(), message),
            }
        }
    }
}
