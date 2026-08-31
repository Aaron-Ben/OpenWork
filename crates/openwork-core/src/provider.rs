use openwork_models::model::ModelCapabilities;
use openwork_models::provider::{ModelTier, ProviderKind};
use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderPreset {
    pub id: &'static str,
    pub name: &'static str,
    pub base_url: &'static str,
    pub kind: ProviderKind,
    pub models: &'static [ProviderPresetModel],
    pub website_url: &'static str,
    pub api_key_url: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderPresetModel {
    pub model_id: &'static str,
    pub model_tier: ModelTier,
    pub capabilities: ModelCapabilities,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderTestResult {
    pub success: bool,
    pub message: String,
}

impl ProviderTestResult {
    pub(crate) fn failed(message: String) -> Self {
        Self {
            success: false,
            message,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderIndex {
    pub providers: Vec<openwork_models::provider::ProviderProfile>,
}

pub(crate) const BUILTIN_PRESETS: &[ProviderPreset] = &[
    ProviderPreset {
        id: "openai",
        name: "OpenAI",
        base_url: "https://api.openai.com",
        kind: ProviderKind::Openai,
        models: &[ProviderPresetModel {
            model_id: "gpt-5.1",
            model_tier: ModelTier::Plus,
            capabilities: ModelCapabilities {
                context_window_tokens: 400_000,
                max_output_tokens: 32_768,
                max_reasoning_tokens: None,
                accepts_data_blocks: true,
            },
        }],
        website_url: "https://platform.openai.com",
        api_key_url: "https://platform.openai.com/api-keys",
    },
    ProviderPreset {
        id: "anthropic",
        name: "Anthropic",
        base_url: "https://api.anthropic.com",
        kind: ProviderKind::Anthropic,
        models: &[ProviderPresetModel {
            model_id: "claude-sonnet-4-5",
            model_tier: ModelTier::Plus,
            capabilities: ModelCapabilities {
                context_window_tokens: 200_000,
                max_output_tokens: 32_768,
                max_reasoning_tokens: None,
                accepts_data_blocks: true,
            },
        }],
        website_url: "https://www.anthropic.com",
        api_key_url: "https://console.anthropic.com/settings/keys",
    },
    ProviderPreset {
        id: "deepseek",
        name: "DeepSeek",
        base_url: "https://api.deepseek.com",
        kind: ProviderKind::Deepseek,
        models: &[ProviderPresetModel {
            model_id: "deepseek-v4-flash",
            model_tier: ModelTier::Plus,
            capabilities: ModelCapabilities {
                context_window_tokens: 1_048_576,
                max_output_tokens: 32_768,
                max_reasoning_tokens: None,
                accepts_data_blocks: false,
            },
        }],
        website_url: "https://www.deepseek.com",
        api_key_url: "https://platform.deepseek.com/api_keys",
    },
    ProviderPreset {
        id: "kimi",
        name: "Kimi (Moonshot)",
        base_url: "https://api.moonshot.cn/v1",
        kind: ProviderKind::Kimi,
        models: &[ProviderPresetModel {
            model_id: "kimi-k2.6",
            model_tier: ModelTier::Plus,
            capabilities: ModelCapabilities {
                context_window_tokens: 262_144,
                max_output_tokens: 32_768,
                max_reasoning_tokens: None,
                accepts_data_blocks: true,
            },
        }],
        website_url: "https://www.moonshot.cn",
        api_key_url: "https://platform.moonshot.cn/console/api-keys",
    },
    ProviderPreset {
        id: "qwen",
        name: "Qwen (DashScope)",
        base_url: "https://dashscope.aliyuncs.com/compatible-mode/v1",
        kind: ProviderKind::Qwen,
        models: &[ProviderPresetModel {
            model_id: "qwen-plus",
            model_tier: ModelTier::Plus,
            capabilities: ModelCapabilities {
                context_window_tokens: 1_000_000,
                max_output_tokens: 32_768,
                max_reasoning_tokens: Some(81_920),
                accepts_data_blocks: false,
            },
        }],
        website_url: "https://www.aliyun.com/product/bailian",
        api_key_url: "https://bailian.console.aliyun.com",
    },
    ProviderPreset {
        id: "glm",
        name: "GLM (Zhipu)",
        base_url: "https://open.bigmodel.cn/api/paas/v4",
        kind: ProviderKind::Glm,
        models: &[ProviderPresetModel {
            model_id: "glm-5.2",
            model_tier: ModelTier::Plus,
            capabilities: ModelCapabilities {
                context_window_tokens: 1_000_000,
                max_output_tokens: 32_768,
                max_reasoning_tokens: None,
                accepts_data_blocks: false,
            },
        }],
        website_url: "https://www.zhipuai.cn",
        api_key_url: "https://open.bigmodel.cn/usercenter/apikeys",
    },
];

#[cfg(test)]
mod tests {
    use super::BUILTIN_PRESETS;

    #[test]
    fn builtin_presets_exclude_local_and_custom_providers() {
        let ids = BUILTIN_PRESETS
            .iter()
            .map(|preset| preset.id)
            .collect::<Vec<_>>();
        for unsupported in ["ollama", "lmstudio", "official", "custom"] {
            assert!(!ids.contains(&unsupported));
        }
    }
}
