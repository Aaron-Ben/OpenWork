use std::sync::Arc;

use openwork_models::ProviderFactory;
use openwork_protocol::provider::{
    ApiCredential, ModelTier, ProviderInput, ProviderKind, ProviderProfile, ProviderRepository,
    ProviderRepositoryError, ProviderRuntimeConfig,
};
use serde::Serialize;

use crate::ApplicationError;

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
pub struct ProviderTestResult {
    pub success: bool,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderIndex {
    pub providers: Vec<ProviderProfile>,
    pub active_id: Option<String>,
}

const BUILTIN_PRESETS: &[ProviderPreset] = &[
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

pub struct ProviderApplicationService {
    repository: Arc<dyn ProviderRepository>,
    factory: ProviderFactory,
}

impl ProviderApplicationService {
    pub(crate) fn new(repository: Arc<dyn ProviderRepository>, factory: ProviderFactory) -> Self {
        Self {
            repository,
            factory,
        }
    }

    pub async fn list(&self) -> Result<ProviderIndex, ApplicationError> {
        let providers = self.repository.list_profiles().await?;
        let active_id = self.repository.active_id().await?;
        Ok(ProviderIndex {
            providers,
            active_id,
        })
    }

    pub fn presets(&self) -> Vec<ProviderPreset> {
        BUILTIN_PRESETS.to_vec()
    }

    pub async fn create(&self, input: ProviderInput) -> Result<ProviderProfile, ApplicationError> {
        Ok(self.repository.create(input).await?)
    }

    pub async fn update(
        &self,
        id: &str,
        input: ProviderInput,
    ) -> Result<ProviderProfile, ApplicationError> {
        Ok(self.repository.update(id, input).await?)
    }

    pub async fn delete(&self, id: &str) -> Result<(), ApplicationError> {
        Ok(self.repository.delete(id).await?)
    }

    pub async fn activate(&self, id: &str) -> Result<(), ApplicationError> {
        Ok(self.repository.activate(id).await?)
    }

    pub async fn test(
        &self,
        id: Option<String>,
        input: Option<ProviderInput>,
        model: &str,
    ) -> Result<ProviderTestResult, ApplicationError> {
        let config = if let Some(id) = id {
            self.repository
                .load_runtime(&id)
                .await?
                .ok_or(ProviderRepositoryError::NotFound { id })?
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
            return Err(ApplicationError::new(
                crate::ApplicationErrorCode::InvalidRequest,
                "Either provider id or draft input is required",
            ));
        };

        match self.factory.test(&config, model).await {
            Ok(()) => Ok(ProviderTestResult {
                success: true,
                message: "Connectivity OK".to_string(),
            }),
            Err(error) => Ok(ProviderTestResult::failed(error.to_string())),
        }
    }
}

impl ProviderTestResult {
    fn failed(message: String) -> Self {
        Self {
            success: false,
            message,
        }
    }
}

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
