use std::borrow::Borrow;
use std::fmt::{Display, Formatter};

use openwork_models::model::ToolDefinition as ModelToolDefinition;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ToolId(String);

impl ToolId {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn new_static(value: &'static str) -> Self {
        Self(value.to_string())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Borrow<str> for ToolId {
    fn borrow(&self) -> &str {
        self.as_str()
    }
}

impl Display for ToolId {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolRisk {
    ReadOnly,
    WorkspaceMutation,
    ProcessExecution,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolDefinition {
    pub id: ToolId,
    pub description: String,
    pub input_schema: Value,
    pub risk_hint: ToolRisk,
}

impl ToolDefinition {
    pub fn model_definition(&self) -> ModelToolDefinition {
        ModelToolDefinition {
            name: self.id.to_string(),
            description: self.description.clone(),
            parameters: self.input_schema.clone(),
        }
    }
}
