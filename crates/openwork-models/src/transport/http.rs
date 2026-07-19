use std::sync::Arc;

use reqwest::header::HeaderMap;
use serde_json::Value;

#[derive(Debug, Clone)]
pub struct HttpTransport {
    inner: Arc<HttpTransportInner>,
}

#[derive(Debug)]
struct HttpTransportInner {
    client: reqwest::Client,
}

impl HttpTransport {
    pub fn new(client: reqwest::Client) -> Self {
        Self {
            inner: Arc::new(HttpTransportInner { client }),
        }
    }

    pub(crate) async fn post_json(
        &self,
        endpoint: String,
        headers: HeaderMap,
        body: &Value,
    ) -> Result<reqwest::Response, reqwest::Error> {
        self.inner
            .client
            .post(endpoint)
            .headers(headers)
            .json(body)
            .send()
            .await
    }

    #[cfg(test)]
    pub(crate) fn shares_lifecycle_with(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.inner, &other.inner)
    }
}

impl Default for HttpTransport {
    fn default() -> Self {
        Self::new(reqwest::Client::new())
    }
}

#[derive(Clone)]
pub(crate) struct HttpProviderConfig {
    base_url: String,
    api_key: String,
}

impl HttpProviderConfig {
    pub(crate) fn new(base_url: impl Into<String>, api_key: impl Into<String>) -> Self {
        Self {
            base_url: trim_trailing_slash(base_url.into()),
            api_key: api_key.into(),
        }
    }

    pub(crate) fn endpoint(&self, path: &str) -> String {
        format!("{}{}", self.base_url, path)
    }

    pub(crate) fn api_key(&self) -> &str {
        &self.api_key
    }
}

impl std::fmt::Debug for HttpProviderConfig {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("HttpProviderConfig")
            .field("base_url", &self.base_url)
            .field("api_key", &"[REDACTED]")
            .finish()
    }
}

fn trim_trailing_slash(value: String) -> String {
    value.trim_end_matches('/').to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn endpoint_joins_without_duplicate_slashes() {
        let config = HttpProviderConfig::new("https://api.example.com/", "key");
        assert_eq!(
            config.endpoint("/v1/messages"),
            "https://api.example.com/v1/messages"
        );
    }

    #[test]
    fn cloned_transport_shares_client_lifecycle() {
        let transport = HttpTransport::default();
        let clone = transport.clone();

        assert!(transport.shares_lifecycle_with(&clone));
    }
}
