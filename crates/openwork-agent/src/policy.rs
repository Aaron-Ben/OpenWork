use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentPolicy {
    pub max_model_calls: u32,
    pub doom_loop_threshold: usize,
}

impl Default for AgentPolicy {
    fn default() -> Self {
        Self {
            max_model_calls: 20,
            doom_loop_threshold: 3,
        }
    }
}
