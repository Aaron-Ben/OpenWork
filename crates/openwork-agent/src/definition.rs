use serde::{Deserialize, Serialize};

use crate::AgentPolicy;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentDefinition {
    pub name: String,
    pub description: String,
    pub system_prompt: String,
    pub tool_names: Vec<String>,
    pub policy: AgentPolicy,
}

impl Default for AgentDefinition {
    fn default() -> Self {
        Self {
            name: "OpenWork".to_string(),
            description: "Autonomous coding agent".to_string(),
            system_prompt: crate::DEFAULT_SYSTEM_PROMPT.to_string(),
            tool_names: vec![
                "read".to_string(),
                "write".to_string(),
                "edit".to_string(),
                "grep".to_string(),
                "glob".to_string(),
                "list".to_string(),
                "bash".to_string(),
            ],
            policy: AgentPolicy::default(),
        }
    }
}
