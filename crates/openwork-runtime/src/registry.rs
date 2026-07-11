use std::collections::HashMap;

use openwork_protocol::model::{ModelCapability, ModelSpec};
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RegistryConfig {
    #[serde(default)]
    pub models: Vec<ModelSpec>,
    #[serde(default)]
    pub defaults: HashMap<String, String>,
    #[serde(default)]
    pub fallbacks: HashMap<String, Vec<FallbackRule>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FallbackRule {
    pub primary: String,
    #[serde(default)]
    pub fallbacks: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct ModelRegistry {
    models: HashMap<String, ModelSpec>,
    defaults: HashMap<String, String>,
    fallbacks: HashMap<String, Vec<FallbackRule>>,
}

impl ModelRegistry {
    pub fn new(config: RegistryConfig) -> Result<Self, RegistryError> {
        let mut models = HashMap::new();

        for model in config.models {
            let key = model_key(&model.provider, &model.model);
            if models.insert(key.clone(), model).is_some() {
                return Err(RegistryError::DuplicateModel { model: key });
            }
        }

        let registry = Self {
            models,
            defaults: config.defaults,
            fallbacks: config.fallbacks,
        };
        registry.validate_references()?;
        Ok(registry)
    }

    pub fn from_toml_str(input: &str) -> Result<Self, RegistryError> {
        let config = toml::from_str::<RegistryConfig>(input)?;
        Self::new(config)
    }

    pub fn get(&self, provider_qualified_model: &str) -> Option<&ModelSpec> {
        self.models.get(provider_qualified_model)
    }

    pub fn default_for(&self, capability_alias: &str) -> Option<&ModelSpec> {
        self.defaults
            .get(capability_alias)
            .and_then(|model| self.get(model))
    }

    pub fn require_capability(
        &self,
        provider_qualified_model: &str,
        capability: ModelCapability,
    ) -> Result<&ModelSpec, RegistryError> {
        let model =
            self.get(provider_qualified_model)
                .ok_or_else(|| RegistryError::UnknownModel {
                    model: provider_qualified_model.to_string(),
                })?;

        if !model.supports(capability) {
            return Err(RegistryError::UnsupportedCapability {
                model: provider_qualified_model.to_string(),
                capability,
            });
        }

        Ok(model)
    }

    pub fn fallback_chain(&self, capability_alias: &str, primary: &str) -> Vec<&ModelSpec> {
        let mut chain = Vec::new();

        if let Some(primary_model) = self.get(primary) {
            chain.push(primary_model);
        }

        if let Some(rules) = self.fallbacks.get(capability_alias)
            && let Some(rule) = rules.iter().find(|rule| rule.primary == primary)
        {
            chain.extend(
                rule.fallbacks
                    .iter()
                    .filter_map(|fallback| self.get(fallback)),
            );
        }

        chain
    }

    fn validate_references(&self) -> Result<(), RegistryError> {
        for model in self.defaults.values() {
            if !self.models.contains_key(model) {
                return Err(RegistryError::UnknownModel {
                    model: model.clone(),
                });
            }
        }

        for rules in self.fallbacks.values() {
            for rule in rules {
                if !self.models.contains_key(&rule.primary) {
                    return Err(RegistryError::UnknownModel {
                        model: rule.primary.clone(),
                    });
                }
                for fallback in &rule.fallbacks {
                    if !self.models.contains_key(fallback) {
                        return Err(RegistryError::UnknownModel {
                            model: fallback.clone(),
                        });
                    }
                }
            }
        }

        Ok(())
    }
}

#[derive(Debug, Error)]
pub enum RegistryError {
    #[error("failed to parse model registry: {0}")]
    Parse(#[from] toml::de::Error),
    #[error("duplicate model registration: {model}")]
    DuplicateModel { model: String },
    #[error("unknown model: {model}")]
    UnknownModel { model: String },
    #[error("model {model} does not support capability {capability:?}")]
    UnsupportedCapability {
        model: String,
        capability: ModelCapability,
    },
}

fn model_key(provider: &str, model: &str) -> String {
    format!("{provider}:{model}")
}

#[cfg(test)]
mod tests {
    use super::*;

    const REGISTRY: &str = r#"
        [[models]]
        provider = "openai"
        model = "gpt-4.1"
        capabilities = ["chat", "vision"]

        [[models]]
        provider = "qwen"
        model = "qwen-plus"
        capabilities = ["chat", "tool_calling", "streaming"]

        [defaults]
        chat = "openai:gpt-4.1"

        [[fallbacks.chat]]
        primary = "openai:gpt-4.1"
        fallbacks = ["qwen:qwen-plus"]
    "#;

    #[test]
    fn loads_registry_from_toml() {
        let registry = ModelRegistry::from_toml_str(REGISTRY).unwrap();

        assert_eq!(
            registry.default_for("chat").unwrap().model,
            "gpt-4.1".to_string()
        );
        assert!(
            registry
                .require_capability("qwen:qwen-plus", ModelCapability::ToolCalling)
                .is_ok()
        );
    }

    #[test]
    fn rejects_unsupported_capability() {
        let registry = ModelRegistry::from_toml_str(REGISTRY).unwrap();

        assert!(matches!(
            registry.require_capability("qwen:qwen-plus", ModelCapability::Vision),
            Err(RegistryError::UnsupportedCapability { .. })
        ));
    }

    #[test]
    fn returns_fallback_chain() {
        let registry = ModelRegistry::from_toml_str(REGISTRY).unwrap();
        let chain = registry.fallback_chain("chat", "openai:gpt-4.1");

        assert_eq!(chain.len(), 2);
        assert_eq!(chain[0].provider, "openai");
        assert_eq!(chain[1].provider, "qwen");
    }
}
