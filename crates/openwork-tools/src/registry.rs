use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use openwork_models::model::ToolDefinition as ModelToolDefinition;
use thiserror::Error;

use crate::policy::evaluate;
use crate::tool::{DynTool, ToolAdapter};
use crate::{
    PermissionMode, PolicyDecision, Tool, ToolCallContext, ToolDefinition, ToolErrorCode, ToolId,
    ToolInvocation, ToolResult, ToolSessionContext,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolsetConfig {
    tools: Vec<ToolId>,
}

impl ToolsetConfig {
    pub fn new(tools: Vec<ToolId>) -> Self {
        Self { tools }
    }

    pub fn from_names(names: impl IntoIterator<Item = impl Into<String>>) -> Self {
        Self::new(
            names
                .into_iter()
                .map(|name| ToolId::new(name.into()))
                .collect(),
        )
    }

    pub fn tools(&self) -> &[ToolId] {
        &self.tools
    }
}

pub struct ToolRegistryBuilder {
    tools: Vec<Arc<dyn DynTool>>,
}

impl Default for ToolRegistryBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl ToolRegistryBuilder {
    pub fn new() -> Self {
        Self { tools: Vec::new() }
    }

    pub fn register<T: Tool>(mut self, tool: T) -> Self {
        self.tools.push(Arc::new(ToolAdapter::new(tool)));
        self
    }

    pub fn finalize(
        self,
        config: &ToolsetConfig,
        session: ToolSessionContext,
    ) -> Result<FinalizedToolset, ToolRegistryError> {
        let mut registered = HashMap::new();
        for tool in self.tools {
            let definition =
                tool.definition()
                    .map_err(|message| ToolRegistryError::InvalidDefinition {
                        tool: tool.id().to_string(),
                        message,
                    })?;
            validate_definition(&definition)?;
            let id = definition.id.clone();
            if registered.insert(id.clone(), (definition, tool)).is_some() {
                return Err(ToolRegistryError::DuplicateRegistration(id.to_string()));
            }
        }

        let mut selected = HashSet::new();
        let mut definitions = Vec::with_capacity(config.tools.len());
        let mut tools = HashMap::with_capacity(config.tools.len());
        for id in &config.tools {
            if id.as_str().trim().is_empty() {
                return Err(ToolRegistryError::BlankToolId);
            }
            if !selected.insert(id.clone()) {
                return Err(ToolRegistryError::DuplicateSelection(id.to_string()));
            }
            let (definition, tool) = registered
                .remove(id)
                .ok_or_else(|| ToolRegistryError::UnknownTool(id.to_string()))?;
            definitions.push(definition.model_definition());
            tools.insert(id.clone(), FinalizedEntry { definition, tool });
        }

        Ok(FinalizedToolset {
            definitions,
            tools,
            session: Arc::new(session),
        })
    }
}

struct FinalizedEntry {
    definition: ToolDefinition,
    tool: Arc<dyn DynTool>,
}

pub struct FinalizedToolset {
    definitions: Vec<ModelToolDefinition>,
    tools: HashMap<ToolId, FinalizedEntry>,
    session: Arc<ToolSessionContext>,
}

impl FinalizedToolset {
    pub fn definitions(&self) -> &[ModelToolDefinition] {
        &self.definitions
    }

    pub fn resolve(&self, id: &str) -> Option<&ToolDefinition> {
        self.tools.get(id).map(|entry| &entry.definition)
    }

    pub fn validate(
        &self,
        invocation: &ToolInvocation,
    ) -> Result<&ToolDefinition, ToolValidationError> {
        let entry = self
            .tools
            .get(invocation.name.as_str())
            .ok_or_else(|| ToolValidationError::UnknownTool(invocation.name.clone()))?;
        entry
            .tool
            .validate(&invocation.input)
            .map_err(ToolValidationError::InvalidInput)?;
        Ok(&entry.definition)
    }

    pub fn authorize(&self, invocation: &ToolInvocation, mode: PermissionMode) -> PolicyDecision {
        match self.validate(invocation) {
            Ok(definition) => evaluate(mode, definition.risk_hint),
            Err(error) => PolicyDecision::Deny {
                reason: error.to_string(),
            },
        }
    }

    pub async fn call(&self, call: ToolCallContext, invocation: ToolInvocation) -> ToolResult {
        let Some(entry) = self.tools.get(invocation.name.as_str()) else {
            return ToolResult::failed(
                ToolErrorCode::ToolNotFound,
                format!("tool not found: {}", invocation.name),
                false,
            );
        };
        entry.tool.call(&self.session, call, invocation.input).await
    }
}

fn validate_definition(definition: &ToolDefinition) -> Result<(), ToolRegistryError> {
    if definition.id.as_str().trim().is_empty() {
        return Err(ToolRegistryError::BlankToolId);
    }
    if definition.description.trim().is_empty() {
        return Err(ToolRegistryError::BlankDescription(
            definition.id.to_string(),
        ));
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ToolRegistryError {
    #[error("tool id must not be blank")]
    BlankToolId,
    #[error("tool description must not be blank: {0}")]
    BlankDescription(String),
    #[error("duplicate tool registration: {0}")]
    DuplicateRegistration(String),
    #[error("duplicate tool selection: {0}")]
    DuplicateSelection(String),
    #[error("toolset references unknown tool: {0}")]
    UnknownTool(String),
    #[error("invalid definition for tool {tool}: {message}")]
    InvalidDefinition { tool: String, message: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ToolValidationError {
    #[error("tool not found: {0}")]
    UnknownTool(String),
    #[error("invalid tool input: {0}")]
    InvalidInput(String),
}

#[cfg(test)]
mod tests {
    use async_trait::async_trait;
    use schemars::JsonSchema;
    use serde::Deserialize;
    use serde_json::json;
    use tokio_util::sync::CancellationToken;

    use super::*;
    use crate::{PermissionProfile, TextToolOutput, ToolCallId, ToolExecutionError, ToolRisk};

    #[derive(Debug, Deserialize, JsonSchema)]
    struct EchoInput {
        text: String,
    }

    #[derive(Debug, Default)]
    struct EchoTool;

    #[async_trait]
    impl Tool for EchoTool {
        type Input = EchoInput;
        type Output = TextToolOutput;

        fn id(&self) -> ToolId {
            ToolId::new_static("echo")
        }

        fn description(&self) -> &'static str {
            "Echo typed input."
        }

        fn risk(&self) -> crate::ToolRisk {
            ToolRisk::ReadOnly
        }

        async fn execute(
            &self,
            _session: &ToolSessionContext,
            _call: ToolCallContext,
            input: EchoInput,
        ) -> Result<TextToolOutput, ToolExecutionError> {
            Ok(TextToolOutput::new(input.text))
        }
    }

    fn session() -> ToolSessionContext {
        ToolSessionContext::local(
            std::env::temp_dir(),
            PermissionProfile::danger_full_access(),
        )
    }

    #[tokio::test]
    async fn finalized_toolset_is_the_model_and_dispatch_subset() {
        let toolset = ToolRegistryBuilder::new()
            .register(EchoTool)
            .finalize(&ToolsetConfig::from_names(["echo"]), session())
            .expect("toolset");

        assert_eq!(toolset.definitions().len(), 1);
        assert_eq!(toolset.definitions()[0].name, "echo");
        let result = toolset
            .call(
                ToolCallContext::new(ToolCallId::new("call-1"), CancellationToken::new()),
                ToolInvocation::new("echo", json!({"text": "hello"})),
            )
            .await;
        assert_eq!(result.text_content(), "hello");
        let missing = toolset
            .call(
                ToolCallContext::new(ToolCallId::new("call-2"), CancellationToken::new()),
                ToolInvocation::new("missing", json!({})),
            )
            .await;
        assert_eq!(
            missing.error.map(|error| error.code),
            Some(ToolErrorCode::ToolNotFound)
        );
    }

    #[test]
    fn typed_input_drives_schema_and_validation() {
        let toolset = ToolRegistryBuilder::new()
            .register(EchoTool)
            .finalize(&ToolsetConfig::from_names(["echo"]), session())
            .expect("toolset");
        let definition = toolset.resolve("echo").expect("definition");
        assert!(definition.input_schema["properties"]["text"].is_object());
        assert!(matches!(
            toolset.validate(&ToolInvocation::new("echo", json!({}))),
            Err(ToolValidationError::InvalidInput(_))
        ));
    }

    #[test]
    fn finalize_rejects_duplicate_and_unknown_configuration() {
        let duplicate_registration = ToolRegistryBuilder::new()
            .register(EchoTool)
            .register(EchoTool)
            .finalize(&ToolsetConfig::from_names(["echo"]), session());
        assert!(matches!(
            duplicate_registration,
            Err(ToolRegistryError::DuplicateRegistration(id)) if id == "echo"
        ));

        let duplicate_selection = ToolRegistryBuilder::new()
            .register(EchoTool)
            .finalize(&ToolsetConfig::from_names(["echo", "echo"]), session());
        assert!(matches!(
            duplicate_selection,
            Err(ToolRegistryError::DuplicateSelection(id)) if id == "echo"
        ));

        let unknown = ToolRegistryBuilder::new()
            .register(EchoTool)
            .finalize(&ToolsetConfig::from_names(["missing"]), session());
        assert!(matches!(
            unknown,
            Err(ToolRegistryError::UnknownTool(id)) if id == "missing"
        ));
    }
}
