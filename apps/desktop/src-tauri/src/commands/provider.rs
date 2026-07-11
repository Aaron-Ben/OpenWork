use std::sync::Arc;

use openwork_protocol::provider::{
    ApiCredential, ModelTier, ProviderInput, ProviderKind, ProviderProfile, ProviderRepository,
    ProviderRuntimeConfig,
};
use openwork_providers::ProviderFactory;
use serde::Serialize;

#[derive(Clone)]
pub struct ProviderRepositoryState(pub Arc<dyn ProviderRepository>);

/// UI discovery data stays in the desktop host; it is not part of a model adapter.
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
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TestResult {
    pub success: bool,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderIndex {
    pub providers: Vec<ProviderProfile>,
    pub active_id: Option<String>,
}

pub const BUILTIN_PRESETS: &[ProviderPreset] = &[
    ProviderPreset {
        id: "openai",
        name: "OpenAI",
        base_url: "https://api.openai.com",
        kind: ProviderKind::Openai,
        models: &[ProviderPresetModel {
            model_id: "gpt-5.1",
            model_tier: ModelTier::Plus,
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
        }],
        website_url: "https://www.zhipuai.cn",
        api_key_url: "https://open.bigmodel.cn/usercenter/apikeys",
    },
];

#[tauri::command]
pub async fn provider_list(
    repository: tauri::State<'_, ProviderRepositoryState>,
) -> Result<ProviderIndex, String> {
    let providers = repository
        .0
        .list_profiles()
        .await
        .map_err(|error| error.to_string())?;
    let active_id = repository
        .0
        .active_id()
        .await
        .map_err(|error| error.to_string())?;
    Ok(ProviderIndex {
        providers,
        active_id,
    })
}

#[tauri::command]
pub fn provider_presets() -> Vec<ProviderPreset> {
    BUILTIN_PRESETS.to_vec()
}

#[tauri::command]
pub async fn provider_create(
    repository: tauri::State<'_, ProviderRepositoryState>,
    input: ProviderInput,
) -> Result<ProviderProfile, String> {
    repository
        .0
        .create(input)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn provider_update(
    repository: tauri::State<'_, ProviderRepositoryState>,
    id: String,
    input: ProviderInput,
) -> Result<ProviderProfile, String> {
    repository
        .0
        .update(&id, input)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn provider_delete(
    repository: tauri::State<'_, ProviderRepositoryState>,
    id: String,
) -> Result<(), String> {
    repository
        .0
        .delete(&id)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn provider_activate(
    repository: tauri::State<'_, ProviderRepositoryState>,
    id: String,
) -> Result<(), String> {
    repository
        .0
        .activate(&id)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn provider_test(
    repository: tauri::State<'_, ProviderRepositoryState>,
    provider_factory: tauri::State<'_, ProviderFactory>,
    id: Option<String>,
    input: Option<ProviderInput>,
    model: String,
) -> Result<TestResult, String> {
    let config = if let Some(id) = id {
        match repository.0.load_runtime(&id).await {
            Ok(Some(config)) => config,
            Ok(None) => {
                return Ok(TestResult {
                    success: false,
                    message: format!("Provider not found: {id}"),
                });
            }
            Err(error) => {
                return Ok(TestResult {
                    success: false,
                    message: error.to_string(),
                });
            }
        }
    } else if let Some(input) = input {
        ProviderRuntimeConfig {
            profile: ProviderProfile {
                id: "draft".to_string(),
                name: input.name,
                base_url: input.base_url,
                kind: input.kind,
                models: input.models,
                enabled: input.enabled,
            },
            credential: ApiCredential::new(input.api_key),
            adapter_options: input.extra_body,
        }
    } else {
        return Ok(TestResult {
            success: false,
            message: "Either provider id or draft input is required".to_string(),
        });
    };
    match provider_factory.test(&config, &model).await {
        Ok(()) => Ok(TestResult {
            success: true,
            message: "Connectivity OK".to_string(),
        }),
        Err(error) => Ok(TestResult {
            success: false,
            message: error.to_string(),
        }),
    }
}
