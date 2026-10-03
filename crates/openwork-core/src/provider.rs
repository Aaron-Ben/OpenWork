//! Provider 预设与界面视图。预设是随应用打包的数据（`provider_presets.json`），不是代码；
//! 模型能力从 `openwork_models::catalog` 解析。

use std::sync::OnceLock;

use openwork_models::catalog::{ModelInfo, resolve_model_info};
use openwork_models::provider::ProviderProfile;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderPreset {
    pub id: String,
    pub name: String,
    pub base_url: String,
    pub models: Vec<ProviderPresetModel>,
    pub website_url: String,
    pub api_key_url: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderPresetModel {
    pub model_id: String,
    #[serde(flatten)]
    pub info: ModelInfo,
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
    pub providers: Vec<ProviderEntry>,
}

/// 一个已配置的 Provider，加上每个模型从目录解析出的显示名、能力与推理档位。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderEntry {
    #[serde(flatten)]
    pub profile: ProviderProfile,
    pub resolved_models: Vec<ResolvedModelView>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedModelView {
    pub model_id: String,
    pub enabled: bool,
    #[serde(flatten)]
    pub info: ModelInfo,
}

impl ProviderEntry {
    pub(crate) fn new(profile: ProviderProfile) -> Self {
        let resolved_models = profile
            .settings
            .models
            .iter()
            .map(|model| {
                let mut info = resolve_model_info(&model.model_id, model.capabilities);
                // 配置中写的显示名优先，供目录以外的模型使用。
                if let Some(display_name) = &model.display_name {
                    info.display_name = display_name.clone();
                }
                ResolvedModelView {
                    model_id: model.model_id.clone(),
                    enabled: model.enabled,
                    info,
                }
            })
            .collect();
        Self {
            profile,
            resolved_models,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PresetFile {
    presets: Vec<PresetEntry>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PresetEntry {
    id: String,
    name: String,
    base_url: String,
    website_url: String,
    api_key_url: String,
    models: Vec<String>,
}

fn preset_entries() -> &'static [PresetEntry] {
    static PRESETS: OnceLock<Vec<PresetEntry>> = OnceLock::new();
    PRESETS.get_or_init(|| {
        let file: PresetFile = serde_json::from_str(include_str!("../provider_presets.json"))
            .expect("provider_presets.json is bundled at build time and checked by a unit test");
        file.presets
    })
}

/// 内置预设，模型元数据已从模型目录解析。
pub(crate) fn builtin_presets() -> Vec<ProviderPreset> {
    preset_entries()
        .iter()
        .map(|entry| ProviderPreset {
            id: entry.id.clone(),
            name: entry.name.clone(),
            base_url: entry.base_url.clone(),
            website_url: entry.website_url.clone(),
            api_key_url: entry.api_key_url.clone(),
            models: entry
                .models
                .iter()
                .map(|model_id| ProviderPresetModel {
                    model_id: model_id.clone(),
                    info: resolve_model_info(model_id, None),
                })
                .collect(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use openwork_models::catalog::{CapabilitySource, resolve_model_info};

    use super::preset_entries;

    #[test]
    fn every_preset_model_is_in_the_catalog() {
        assert!(!preset_entries().is_empty());
        for preset in preset_entries() {
            for model_id in &preset.models {
                assert_eq!(
                    resolve_model_info(model_id, None).source,
                    CapabilitySource::Catalog,
                    "{}/{model_id}",
                    preset.id
                );
            }
        }
    }

    #[test]
    fn preset_ids_cannot_contain_the_model_reference_separator() {
        for preset in preset_entries() {
            assert!(!preset.id.contains('/'), "{}", preset.id);
        }
    }
}
