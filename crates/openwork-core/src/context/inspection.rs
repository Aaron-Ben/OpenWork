use openwork_models::model::{ContentBlock, Role, ToolDefinition};
use serde::{Deserialize, Serialize};

pub const CONTEXT_WINDOW_INSPECTION_SCHEMA_VERSION: u32 = 2;

/// A read-only preview of the three materialized regions resolved from current sources.
///
/// This value is assembled from current authoritative sources. It is not a
/// persisted copy of a provider request and does not include provider framing.
/// An already-running Turn may retain the System Context resolved at Turn start.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextWindowInspection {
    pub schema_version: u32,
    pub session_id: String,
    pub current_turn_id: Option<String>,
    pub resolved_model_name: String,
    pub system_context: Vec<ContextInspectionSystemPart>,
    pub conversation: Vec<ContextInspectionMessage>,
    pub tool_surface: Vec<ToolDefinition>,
    pub budget: ContextInspectionBudget,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextInspectionSystemPart {
    pub source_key: String,
    pub content: Vec<ContentBlock>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextInspectionMessage {
    pub message_id: String,
    pub turn_id: Option<String>,
    pub role: Role,
    pub content: Vec<ContentBlock>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextInspectionBudget {
    pub system_context_tokens: u64,
    pub conversation_tokens: u64,
    pub tool_surface_tokens: u64,
    pub estimated_input_tokens: u64,
    pub reserved_output_tokens: Option<u32>,
    pub auto_compaction_threshold_percent: u8,
}
