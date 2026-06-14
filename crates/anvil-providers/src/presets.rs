use serde::Serialize;

use crate::provider_config::ProviderKind;

/// 内置 provider 预设。仅云端 API-key provider —— 不含本地模型(lmstudio/ollama)与官方登录。
/// 编译期常量,只序列化发给前端,无需 Deserialize(借用字段不支持)。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderPreset {
    pub id: &'static str,
    pub name: &'static str,
    pub base_url: &'static str,
    pub kind: ProviderKind,
    pub models: &'static [&'static str],
    pub website_url: &'static str,
    pub api_key_url: &'static str,
}

/// 云端 API-key only 预设。新增厂商只需在此追加一条。
pub const BUILTIN_PRESETS: &[ProviderPreset] = &[
    ProviderPreset {
        id: "openai",
        name: "OpenAI",
        base_url: "https://api.openai.com",
        kind: ProviderKind::Openai,
        models: &["gpt-5.1"],
        website_url: "https://platform.openai.com",
        api_key_url: "https://platform.openai.com/api-keys",
    },
    ProviderPreset {
        id: "anthropic",
        name: "Anthropic",
        base_url: "https://api.anthropic.com",
        kind: ProviderKind::Anthropic,
        models: &["claude-sonnet-4-5"],
        website_url: "https://www.anthropic.com",
        api_key_url: "https://console.anthropic.com/settings/keys",
    },
    ProviderPreset {
        id: "deepseek",
        name: "DeepSeek",
        base_url: "https://api.deepseek.com",
        kind: ProviderKind::Deepseek,
        models: &["deepseek-v4-flash"],
        website_url: "https://www.deepseek.com",
        api_key_url: "https://platform.deepseek.com/api_keys",
    },
    ProviderPreset {
        id: "kimi",
        name: "Kimi (Moonshot)",
        base_url: "https://api.moonshot.cn",
        kind: ProviderKind::Kimi,
        models: &["kimi-k2.6"],
        website_url: "https://www.moonshot.cn",
        api_key_url: "https://platform.moonshot.cn/console/api-keys",
    },
    ProviderPreset {
        id: "qwen",
        name: "Qwen (DashScope)",
        base_url: "https://dashscope.aliyuncs.com/compatible-mode/v1",
        kind: ProviderKind::Qwen,
        models: &["qwen-plus"],
        website_url: "https://www.aliyun.com/product/bailian",
        api_key_url: "https://bailian.console.aliyun.com",
    },
    ProviderPreset {
        id: "glm",
        name: "GLM (Zhipu)",
        base_url: "https://open.bigmodel.cn/api/paas/v4",
        kind: ProviderKind::Glm,
        models: &["glm-5.2"],
        website_url: "https://www.zhipuai.cn",
        api_key_url: "https://open.bigmodel.cn/usercenter/apikeys",
    },
    ProviderPreset {
        id: "custom",
        name: "Custom",
        base_url: "",
        kind: ProviderKind::OpenaiCompatible,
        models: &[],
        website_url: "",
        api_key_url: "",
    },
];

pub fn presets() -> &'static [ProviderPreset] {
    BUILTIN_PRESETS
}

pub fn find_preset(id: &str) -> Option<&'static ProviderPreset> {
    BUILTIN_PRESETS.iter().find(|preset| preset.id == id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presets_exclude_local_and_official() {
        let ids: Vec<&str> = BUILTIN_PRESETS.iter().map(|p| p.id).collect();
        assert!(!ids.contains(&"ollama"));
        assert!(!ids.contains(&"lmstudio"));
        assert!(!ids.contains(&"official"));
        assert!(!ids.contains(&"openai-official"));
    }

    #[test]
    fn presets_all_use_api_key_cloud_providers() {
        for preset in BUILTIN_PRESETS {
            assert!(matches!(
                preset.kind,
                ProviderKind::Openai
                    | ProviderKind::Glm
                    | ProviderKind::Kimi
                    | ProviderKind::Deepseek
                    | ProviderKind::Qwen
                    | ProviderKind::Anthropic
                    | ProviderKind::OpenaiCompatible
            ));
        }
    }

    #[test]
    fn find_preset_returns_match() {
        assert_eq!(find_preset("deepseek").unwrap().name, "DeepSeek");
        assert!(find_preset("ollama").is_none());
    }
}
