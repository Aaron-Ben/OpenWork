use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolInvocation {
    pub name: String,
    pub input: Value,
}

impl ToolInvocation {
    pub fn new(name: impl Into<String>, input: Value) -> Self {
        Self {
            name: name.into(),
            input,
        }
    }
}
