use openwork_tools::PermissionMode;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentPolicy {
    pub max_model_calls: u32,
    pub doom_loop_threshold: usize,
    pub permission_mode: PermissionMode,
}

impl Default for AgentPolicy {
    fn default() -> Self {
        Self {
            max_model_calls: 20,
            doom_loop_threshold: 3,
            permission_mode: PermissionMode::Ask,
        }
    }
}
