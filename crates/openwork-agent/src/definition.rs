use openwork_sandbox::SandboxMode;
use serde::{Deserialize, Serialize};

use crate::AgentPolicy;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentDefinition {
    pub name: String,
    pub description: String,
    pub system_prompt: String,
    pub tool_names: Vec<String>,
    pub policy: AgentPolicy,
    /// 这个角色能用的最宽沙箱模式（multi-agent.md §4）。作为子 Agent 派生时，生效模式取
    /// 父会话模式与它中较窄者，委派不能变成放宽权限的途径（permissions.md §13.3）。
    pub sandbox_ceiling: SandboxMode,
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
            sandbox_ceiling: SandboxMode::Auto,
        }
    }
}
