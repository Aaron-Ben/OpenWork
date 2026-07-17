use openwork_models::model::ToolDefinition as ModelToolDefinition;
use openwork_tools::ToolCatalog;
use thiserror::Error;

use crate::{AgentDefinition, AgentPolicy};

#[derive(Debug, Clone)]
pub struct Agent {
    definition: AgentDefinition,
    tools: Vec<ModelToolDefinition>,
}

impl Agent {
    pub fn definition(&self) -> &AgentDefinition {
        &self.definition
    }

    pub fn system_prompt(&self) -> &str {
        &self.definition.system_prompt
    }

    pub fn tool_definitions(&self) -> &[ModelToolDefinition] {
        &self.tools
    }

    pub fn policy(&self) -> &AgentPolicy {
        &self.definition.policy
    }
}

pub struct AgentBuilder {
    definition: AgentDefinition,
}

impl AgentBuilder {
    pub fn new(definition: AgentDefinition) -> Self {
        Self { definition }
    }

    pub fn build(self, catalog: &ToolCatalog) -> Result<Agent, AgentBuildError> {
        if self.definition.name.trim().is_empty() {
            return Err(AgentBuildError::BlankName);
        }
        if self.definition.system_prompt.trim().is_empty() {
            return Err(AgentBuildError::BlankSystemPrompt);
        }
        if self.definition.policy.max_model_calls == 0 {
            return Err(AgentBuildError::InvalidMaxModelCalls);
        }
        if self.definition.policy.doom_loop_threshold < 2 {
            return Err(AgentBuildError::InvalidDoomLoopThreshold);
        }

        let tools = self
            .definition
            .tool_names
            .iter()
            .map(|name| {
                catalog
                    .resolve(name)
                    .map(|tool| tool.model_definition())
                    .ok_or_else(|| AgentBuildError::UnknownTool(name.clone()))
            })
            .collect::<Result<Vec<_>, _>>()?;

        Ok(Agent {
            definition: self.definition,
            tools,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum AgentBuildError {
    #[error("agent name must not be blank")]
    BlankName,
    #[error("agent system prompt must not be blank")]
    BlankSystemPrompt,
    #[error("agent max model calls must be positive")]
    InvalidMaxModelCalls,
    #[error("agent doom loop threshold must be at least two")]
    InvalidDoomLoopThreshold,
    #[error("agent references unknown tool: {0}")]
    UnknownTool(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_an_immutable_definition_from_the_tool_catalog() {
        let catalog = ToolCatalog::builtin().expect("builtin catalog");
        let agent = AgentBuilder::new(AgentDefinition::default())
            .build(&catalog)
            .expect("agent");

        assert_eq!(agent.definition().name, "OpenWork");
        assert_eq!(agent.tool_definitions().len(), 7);
        assert_eq!(agent.policy().max_model_calls, 20);
    }

    #[test]
    fn rejects_an_unknown_tool_during_build() {
        let catalog = ToolCatalog::builtin().expect("builtin catalog");
        let mut definition = AgentDefinition::default();
        definition.tool_names.push("missing".to_string());

        assert!(matches!(
            AgentBuilder::new(definition).build(&catalog),
            Err(AgentBuildError::UnknownTool(name)) if name == "missing"
        ));
    }
}
