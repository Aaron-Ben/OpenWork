use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use super::ProviderKind;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelTier {
    Lite,
    Plus,
    Pro,
}

impl ModelTier {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Lite => "lite",
            Self::Plus => "plus",
            Self::Pro => "pro",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderModel {
    pub model_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    pub model_tier: ModelTier,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderProfile {
    pub id: String,
    pub name: String,
    pub base_url: String,
    pub kind: ProviderKind,
    pub models: Vec<ProviderModel>,
    pub enabled: bool,
}

#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderInput {
    pub name: String,
    pub base_url: String,
    pub api_key: String,
    pub kind: ProviderKind,
    #[serde(default)]
    pub models: Vec<ProviderModel>,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extra_body: Option<Map<String, Value>>,
}

impl std::fmt::Debug for ProviderInput {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ProviderInput")
            .field("name", &self.name)
            .field("base_url", &self.base_url)
            .field("api_key", &"[REDACTED]")
            .field("kind", &self.kind)
            .field("models", &self.models)
            .field("enabled", &self.enabled)
            .field("extra_body", &self.extra_body)
            .finish()
    }
}

fn default_enabled() -> bool {
    true
}

#[derive(Clone, PartialEq, Eq)]
pub struct ApiCredential(String);

impl ApiCredential {
    pub fn new(secret: impl Into<String>) -> Self {
        Self(secret.into())
    }
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Debug for ApiCredential {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("ApiCredential([REDACTED])")
    }
}

#[derive(Clone, PartialEq)]
pub struct ProviderRuntimeConfig {
    pub profile: ProviderProfile,
    pub credential: ApiCredential,
    pub adapter_options: Option<Map<String, Value>>,
}

impl std::fmt::Debug for ProviderRuntimeConfig {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ProviderRuntimeConfig")
            .field("profile", &self.profile)
            .field("credential", &self.credential)
            .field(
                "adapter_options",
                &self.adapter_options.as_ref().map(|_| "[PRESENT]"),
            )
            .finish()
    }
}
