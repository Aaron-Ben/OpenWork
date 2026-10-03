use std::collections::BTreeMap;
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
    http_headers: BTreeMap<String, String>,
    query_params: BTreeMap<String, String>,
}

impl HttpProviderConfig {
    pub(crate) fn new(
        base_url: impl Into<String>,
        api_key: impl Into<String>,
        http_headers: BTreeMap<String, String>,
        query_params: BTreeMap<String, String>,
    ) -> Self {
        Self {
            base_url: trim_trailing_slash(base_url.into()),
            api_key: api_key.into(),
            http_headers,
            query_params,
        }
    }

    /// `{base_url}{path}`，再附上配置中的查询参数。
    pub(crate) fn endpoint(&self, path: &str) -> String {
        let endpoint = format!("{}{}", self.base_url, path);
        if self.query_params.is_empty() {
            return endpoint;
        }
        let query = self
            .query_params
            .iter()
            .map(|(key, value)| format!("{}={}", encode_query(key), encode_query(value)))
            .collect::<Vec<_>>()
            .join("&");
        let separator = if endpoint.contains('?') { '&' } else { '?' };
        format!("{endpoint}{separator}{query}")
    }

    pub(crate) fn api_key(&self) -> &str {
        &self.api_key
    }

    pub(crate) fn http_headers(&self) -> &BTreeMap<String, String> {
        &self.http_headers
    }
}

impl std::fmt::Debug for HttpProviderConfig {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("HttpProviderConfig")
            .field("base_url", &self.base_url)
            .field("api_key", &"[REDACTED]")
            .field(
                "http_headers",
                &self.http_headers.keys().collect::<Vec<_>>(),
            )
            .field(
                "query_params",
                &self.query_params.keys().collect::<Vec<_>>(),
            )
            .finish()
    }
}

/// 查询参数的百分号编码：只保留 RFC 3986 的非保留字符。
fn encode_query(value: &str) -> String {
    value
        .bytes()
        .map(|byte| match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                (byte as char).to_string()
            }
            _ => format!("%{byte:02X}"),
        })
        .collect()
}

fn trim_trailing_slash(value: String) -> String {
    value.trim_end_matches('/').to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn endpoint_joins_without_duplicate_slashes() {
        let config = HttpProviderConfig::new(
            "https://api.example.com/v1/",
            "key",
            BTreeMap::new(),
            BTreeMap::new(),
        );
        assert_eq!(
            config.endpoint("/responses"),
            "https://api.example.com/v1/responses"
        );
    }

    #[test]
    fn endpoint_appends_encoded_query_params() {
        let config = HttpProviderConfig::new(
            "https://api.example.com/v1",
            "key",
            BTreeMap::new(),
            BTreeMap::from([("api-version".to_string(), "2025 preview".to_string())]),
        );
        assert_eq!(
            config.endpoint("/responses"),
            "https://api.example.com/v1/responses?api-version=2025%20preview"
        );
    }

    #[test]
    fn cloned_transport_shares_client_lifecycle() {
        let transport = HttpTransport::default();
        let clone = transport.clone();

        assert!(transport.shares_lifecycle_with(&clone));
    }
}
