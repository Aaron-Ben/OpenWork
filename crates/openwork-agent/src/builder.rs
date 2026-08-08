use openwork_tools::ToolsetConfig;
use thiserror::Error;

use crate::{AgentDefinition, AgentPolicy};

#[derive(Debug, Clone)]
pub struct Agent {
    definition: AgentDefinition,
    toolset: ToolsetConfig,
}

impl Agent {
    pub fn definition(&self) -> &AgentDefinition {
        &self.definition
    }

    pub fn system_prompt(&self) -> &str {
        &self.definition.system_prompt
    }

    pub fn toolset_config(&self) -> &ToolsetConfig {
        &self.toolset
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

    pub fn build(self) -> Result<Agent, AgentBuildError> {
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

        let toolset = ToolsetConfig::from_names(self.definition.tool_names.iter().cloned());

        Ok(Agent {
            definition: self.definition,
            toolset,
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_an_immutable_definition_and_toolset_config() {
        let agent = AgentBuilder::new(AgentDefinition::default())
            .build()
            .expect("agent");

        assert_eq!(agent.definition().name, "OpenWork");
        assert_eq!(agent.toolset_config().tools().len(), 7);
        assert_eq!(agent.policy().max_model_calls, 20);
    }

    #[test]
    fn keeps_tool_selection_declarative_until_runtime_finalization() {
        let mut definition = AgentDefinition::default();
        definition.tool_names.push("missing".to_string());
        let agent = AgentBuilder::new(definition).build().expect("agent");
        assert_eq!(
            agent.toolset_config().tools().last().map(|id| id.as_str()),
            Some("missing")
        );
    }

    #[test]
    fn explorer_is_a_bounded_read_only_agent() {
        let definition = crate::explorer_definition();

        assert_eq!(definition.name, "explorer");
        assert_eq!(
            definition.tool_names,
            ["read", "grep", "glob", "list", "bash"]
        );
        assert_eq!(definition.policy.max_model_calls, 15);
        assert_eq!(definition.policy.doom_loop_threshold, 3);
        assert!(definition.system_prompt.contains("git status"));
        assert!(definition.system_prompt.contains("Do not modify"));
        assert!(definition.system_prompt.contains("Do not ask questions"));
    }
}
