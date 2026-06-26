use openwork_protocol::ai::LlmProvider;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::{
    AnthropicProvider, DeepSeekProvider, GlmProvider, KimiProvider, OpenAiCompatibleChatProvider,
    OpenAiProvider, QwenProvider, config::HttpProviderConfig,
};

/// 厂商类型。配置层表达业务上选择了哪家 provider,底层 adapter 再决定复用哪种协议。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderKind {
    /// OpenAI Responses API: POST {base}/v1/responses, Authorization: Bearer
    #[serde(alias = "openai_responses")]
    Openai,
    /// Zhipu GLM OpenAI-compatible API: POST {base}/chat/completions, Authorization: Bearer
    Glm,
    /// Moonshot Kimi OpenAI-compatible API with provider-specific thinking support.
    Kimi,
    /// DeepSeek OpenAI-compatible API.
    Deepseek,
    /// Qwen DashScope OpenAI-compatible API.
    Qwen,
    /// Anthropic Messages: POST {base}/v1/messages, x-api-key + anthropic-version
    Anthropic,
    /// User supplied OpenAI-compatible chat endpoint. Kept for generic/self-hosted gateways.
    #[serde(alias = "openai_chat")]
    OpenaiCompatible,
}

impl ProviderKind {
    pub fn as_str(self) -> &'static str {
        match self {
            ProviderKind::Openai => "openai",
            ProviderKind::Glm => "glm",
            ProviderKind::Kimi => "kimi",
            ProviderKind::Deepseek => "deepseek",
            ProviderKind::Qwen => "qwen",
            ProviderKind::Anthropic => "anthropic",
            ProviderKind::OpenaiCompatible => "openai_compatible",
        }
    }
}

/// 新增/更新 provider 的输入(不含 id,id 由 store 生成)。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderInput {
    pub name: String,
    pub base_url: String,
    pub api_key: String,
    pub kind: ProviderKind,
    #[serde(default)]
    pub models: Vec<String>,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extra_body: Option<Map<String, Value>>,
}

fn default_enabled() -> bool {
    true
}

/// 持久化的 provider 配置。data 字段 flatten 到顶层,前端看到扁平 JSON。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderConfig {
    pub id: String,
    #[serde(flatten)]
    pub data: ProviderInput,
}

impl ProviderConfig {
    pub fn new(id: String, data: ProviderInput) -> Self {
        Self { id, data }
    }

    pub fn base_url(&self) -> &str {
        &self.data.base_url
    }

    pub fn api_key(&self) -> &str {
        &self.data.api_key
    }

    pub fn kind(&self) -> ProviderKind {
        self.data.kind
    }

    pub fn extra_body(&self) -> Option<&Map<String, Value>> {
        self.data.extra_body.as_ref()
    }
}

/// 按 kind 把配置构造成具体 provider。新增厂商只需新增 preset/配置,无需改代码。
pub fn build_provider(config: &ProviderConfig) -> Box<dyn LlmProvider + Send + Sync> {
    let http = HttpProviderConfig::new(config.base_url(), config.api_key());
    match config.kind() {
        ProviderKind::Openai => Box::new(OpenAiProvider::new(http)),
        ProviderKind::Glm => {
            let mut provider = GlmProvider::new(http);
            if let Some(extra_body) = config.extra_body() {
                provider = provider.with_extra_body(extra_body.clone());
            }
            Box::new(provider)
        }
        ProviderKind::Kimi => {
            let mut provider = KimiProvider::new(http);
            if let Some(extra_body) = config.extra_body() {
                provider = provider.with_extra_body(extra_body.clone());
            }
            Box::new(provider)
        }
        ProviderKind::Deepseek => {
            let mut provider = DeepSeekProvider::new(http);
            if let Some(extra_body) = config.extra_body() {
                provider = provider.with_extra_body(extra_body.clone());
            }
            Box::new(provider)
        }
        ProviderKind::Qwen => {
            let mut provider = QwenProvider::new(http);
            if let Some(extra_body) = config.extra_body() {
                provider = provider.with_extra_body(extra_body.clone());
            }
            Box::new(provider)
        }
        ProviderKind::Anthropic => Box::new(AnthropicProvider::new(http)),
        ProviderKind::OpenaiCompatible => {
            let mut provider = OpenAiCompatibleChatProvider::new(http);
            if let Some(extra_body) = config.extra_body() {
                provider = provider.with_extra_body(extra_body.clone());
            }
            Box::new(provider)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_input(kind: ProviderKind) -> ProviderInput {
        ProviderInput {
            name: "DeepSeek".to_string(),
            base_url: "https://api.deepseek.com".to_string(),
            api_key: "sk-test".to_string(),
            kind,
            models: vec!["deepseek-chat".to_string()],
            enabled: true,
            extra_body: None,
        }
    }

    #[test]
    fn kind_serializes_snake_case() {
        for (kind, expected) in [
            (ProviderKind::Openai, "openai"),
            (ProviderKind::Glm, "glm"),
            (ProviderKind::Kimi, "kimi"),
            (ProviderKind::Deepseek, "deepseek"),
            (ProviderKind::Qwen, "qwen"),
            (ProviderKind::Anthropic, "anthropic"),
            (ProviderKind::OpenaiCompatible, "openai_compatible"),
        ] {
            let json = serde_json::to_string(&kind).unwrap();
            assert_eq!(json, format!("\"{expected}\""));
        }
    }

    #[test]
    fn config_round_trips_with_camel_case() {
        let config =
            ProviderConfig::new("prov-1".to_string(), sample_input(ProviderKind::Deepseek));
        let json = serde_json::to_string(&config).unwrap();

        assert!(json.contains("\"id\":\"prov-1\""));
        assert!(json.contains("\"baseUrl\":\"https://api.deepseek.com\""));
        assert!(json.contains("\"apiKey\":\"sk-test\""));
        assert!(json.contains("\"kind\":\"deepseek\""));

        let back: ProviderConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(back, config);
    }

    #[test]
    fn config_defaults_enabled_when_missing() {
        let json = r#"{"id":"x","name":"n","baseUrl":"u","apiKey":"k","kind":"anthropic"}"#;
        let config: ProviderConfig = serde_json::from_str(json).unwrap();
        assert!(config.data.enabled);
        assert!(config.data.models.is_empty());
        assert!(config.data.extra_body.is_none());
    }

    #[test]
    fn legacy_protocol_kinds_still_deserialize() {
        let chat: ProviderKind = serde_json::from_str("\"openai_chat\"").unwrap();
        let responses: ProviderKind = serde_json::from_str("\"openai_responses\"").unwrap();

        assert_eq!(chat, ProviderKind::OpenaiCompatible);
        assert_eq!(responses, ProviderKind::Openai);
    }

    #[test]
    fn build_provider_accepts_all_supported_kinds() {
        for kind in [
            ProviderKind::Openai,
            ProviderKind::Glm,
            ProviderKind::Kimi,
            ProviderKind::Deepseek,
            ProviderKind::Qwen,
            ProviderKind::Anthropic,
            ProviderKind::OpenaiCompatible,
        ] {
            let config = ProviderConfig::new("t".to_string(), sample_input(kind));
            let _provider = build_provider(&config);
        }
    }
}
