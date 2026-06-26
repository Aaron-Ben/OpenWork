use openwork_protocol::ai::ProviderError;

#[derive(Debug, Clone)]
pub struct HttpProviderConfig {
    pub base_url: String,
    pub api_key: String,
}

impl HttpProviderConfig {
    pub fn new(base_url: impl Into<String>, api_key: impl Into<String>) -> Self {
        Self {
            base_url: trim_trailing_slash(base_url.into()),
            api_key: api_key.into(),
        }
    }

    pub fn from_env(
        base_url: impl Into<String>,
        api_key_env: impl AsRef<str>,
    ) -> Result<Self, ProviderError> {
        let env_name = api_key_env.as_ref();
        let api_key = std::env::var(env_name).map_err(|_| ProviderError::Authentication)?;
        Ok(Self::new(base_url, api_key))
    }

    pub fn endpoint(&self, path: &str) -> String {
        format!("{}{}", self.base_url, path)
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
}
