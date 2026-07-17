use openwork_models::model::ToolDefinition as ModelToolDefinition;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolRisk {
    ReadOnly,
    WorkspaceMutation,
    ProcessExecution,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    pub input_schema: Value,
    pub risk_hint: ToolRisk,
}

impl ToolDefinition {
    pub fn model_definition(&self) -> ModelToolDefinition {
        ModelToolDefinition {
            name: self.name.clone(),
            description: self.description.clone(),
            parameters: self.input_schema.clone(),
        }
    }
}
