//! 随应用打包的模型目录。模型能力与 Provider 连接分开存放，做法来自 Codex 的
//! `models-manager`（`codex-rs/models-manager/models.json` 与 `model_info.rs`）。
//!
//! 查找顺序：Provider 配置中的覆盖 → 目录精确匹配 → 目录最长前缀匹配 → 默认值。
//! 用到默认值时记 warn，调用方应在界面上提示。

use std::sync::OnceLock;

use serde::{Deserialize, Serialize};

use crate::model::ModelCapabilities;

/// 目录中找不到的模型使用的能力。窗口与输出都取保守值：估小了只会提早压缩，估大了会被 Provider 拒绝。
pub const FALLBACK_CAPABILITIES: ModelCapabilities = ModelCapabilities {
    context_window_tokens: 128_000,
    max_output_tokens: 8_192,
    max_reasoning_tokens: None,
    accepts_data_blocks: false,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum CapabilitySource {
    /// Provider 配置中写明的能力。
    Configured,
    /// 模型目录中的条目（精确或最长前缀匹配）。
    Catalog,
    /// 目录中找不到，使用 [`FALLBACK_CAPABILITIES`]。
    Fallback,
}

/// 一个模型的元数据：显示名、能力与推理档位。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelInfo {
    pub display_name: String,
    pub capabilities: ModelCapabilities,
    /// Responses `reasoning.effort` 的可选值，原样发送。空列表表示不发送 `reasoning`。
    pub reasoning_efforts: Vec<String>,
    /// 在 `reasoning_efforts` 中；列表为空时为 `None`。
    pub default_reasoning_effort: Option<String>,
    pub source: CapabilitySource,
}

impl ModelInfo {
    /// 用户选的档位在列表中时用它，否则用默认档位。换模型后旧档位可能不再可用。
    pub fn effective_reasoning_effort(&self, selected: Option<&str>) -> Option<String> {
        selected
            .filter(|effort| self.reasoning_efforts.iter().any(|known| known == effort))
            .map(str::to_string)
            .or_else(|| self.default_reasoning_effort.clone())
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CatalogFile {
    models: Vec<CatalogEntry>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CatalogEntry {
    model_id: String,
    display_name: String,
    #[serde(flatten)]
    capabilities: ModelCapabilities,
    #[serde(default)]
    reasoning_efforts: Vec<String>,
    default_reasoning_effort: Option<String>,
}

fn catalog() -> &'static [CatalogEntry] {
    static CATALOG: OnceLock<Vec<CatalogEntry>> = OnceLock::new();
    CATALOG.get_or_init(|| {
        let file: CatalogFile = serde_json::from_str(include_str!("../catalog.json"))
            .expect("catalog.json is bundled at build time and checked by a unit test");
        file.models
    })
}

/// 解析一个模型的元数据。`configured` 是 Provider 配置中写明的能力，优先级最高；
/// 显示名与推理档位总是来自目录。
pub fn resolve_model_info(model_id: &str, configured: Option<ModelCapabilities>) -> ModelInfo {
    let entry = catalog()
        .iter()
        .find(|entry| entry.model_id == model_id)
        .or_else(|| {
            catalog()
                .iter()
                .filter(|entry| model_id.starts_with(&entry.model_id))
                .max_by_key(|entry| entry.model_id.len())
        });
    let (capabilities, source) = match (configured, entry) {
        (Some(capabilities), _) => (capabilities, CapabilitySource::Configured),
        (None, Some(entry)) => (entry.capabilities, CapabilitySource::Catalog),
        (None, None) => {
            tracing::warn!(model_id, "unknown model, using fallback capabilities");
            (FALLBACK_CAPABILITIES, CapabilitySource::Fallback)
        }
    };
    ModelInfo {
        display_name: entry
            .map_or_else(|| model_id.to_string(), |entry| entry.display_name.clone()),
        capabilities,
        reasoning_efforts: entry
            .map(|entry| entry.reasoning_efforts.clone())
            .unwrap_or_default(),
        default_reasoning_effort: entry.and_then(|entry| entry.default_reasoning_effort.clone()),
        source,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_bundled_entry_is_valid() {
        assert!(!catalog().is_empty());
        for entry in catalog() {
            entry
                .capabilities
                .validate()
                .unwrap_or_else(|error| panic!("{}: {error}", entry.model_id));
            assert!(!entry.display_name.trim().is_empty(), "{}", entry.model_id);
            match &entry.default_reasoning_effort {
                Some(default) => assert!(
                    entry.reasoning_efforts.contains(default),
                    "{}: default {default} is not a listed effort",
                    entry.model_id
                ),
                None => assert!(entry.reasoning_efforts.is_empty(), "{}", entry.model_id),
            }
        }
        FALLBACK_CAPABILITIES
            .validate()
            .expect("fallback capabilities are valid");
    }

    #[test]
    fn resolves_configured_then_exact_then_longest_prefix_then_fallback() {
        let configured = ModelCapabilities {
            context_window_tokens: 10_000,
            max_output_tokens: 1_000,
            max_reasoning_tokens: None,
            accepts_data_blocks: false,
        };
        let overridden = resolve_model_info("glm-5.3", Some(configured));
        assert_eq!(overridden.source, CapabilitySource::Configured);
        assert_eq!(overridden.display_name, "GLM-5.3");
        assert_eq!(
            resolve_model_info("glm-5.3", None).source,
            CapabilitySource::Catalog
        );
        let prefixed = resolve_model_info("glm-5.3-0915", None);
        assert_eq!(prefixed.source, CapabilitySource::Catalog);
        assert_eq!(prefixed.capabilities.context_window_tokens, 1_048_576);
        let unknown = resolve_model_info("someone-elses-model", None);
        assert_eq!(unknown.source, CapabilitySource::Fallback);
        assert_eq!(unknown.capabilities, FALLBACK_CAPABILITIES);
        assert_eq!(unknown.display_name, "someone-elses-model");
        assert!(unknown.reasoning_efforts.is_empty());
    }

    #[test]
    fn an_unlisted_selection_falls_back_to_the_default_effort() {
        let info = resolve_model_info("kimi-k3", None);
        assert_eq!(
            info.effective_reasoning_effort(Some("low")).as_deref(),
            Some("low")
        );
        assert_eq!(
            info.effective_reasoning_effort(Some("none")).as_deref(),
            Some("max")
        );
        assert_eq!(
            info.effective_reasoning_effort(None).as_deref(),
            Some("max")
        );
        let unknown = resolve_model_info("someone-elses-model", None);
        assert_eq!(unknown.effective_reasoning_effort(Some("high")), None);
    }
}
