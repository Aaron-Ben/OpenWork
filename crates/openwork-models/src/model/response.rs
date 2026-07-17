use serde::{Deserialize, Serialize};

use super::{ProviderOpaqueBlock, ToolCallBlock};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FinishReason {
    Stop,
    Length,
    ToolUse,
    ContentFilter,
    Refusal,
    Cancelled,
    Incomplete,
    Unknown(String),
}

impl FinishReason {
    pub fn as_str(&self) -> &str {
        match self {
            Self::Stop => "stop",
            Self::Length => "length",
            Self::ToolUse => "tool_use",
            Self::ContentFilter => "content_filter",
            Self::Refusal => "refusal",
            Self::Cancelled => "cancelled",
            Self::Incomplete => "incomplete",
            Self::Unknown(raw) => raw,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelResponse {
    pub response_id: Option<String>,
    #[serde(default)]
    pub provider_request_id: Option<String>,
    pub model: Option<String>,
    pub text: String,
    pub reasoning_text: Option<String>,
    #[serde(default)]
    pub tool_calls: Vec<ToolCallBlock>,
    #[serde(default)]
    pub provider_opaque_blocks: Vec<ProviderOpaqueBlock>,
    pub finish_reason: FinishReason,
    #[serde(default)]
    pub raw_finish_reason: Option<String>,
    pub usage: Option<TokenUsage>,
}

pub type GenerateResponse = ModelResponse;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TokenUsage {
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub total_tokens: Option<u64>,
    pub cached_input_tokens: Option<u64>,
    pub reasoning_tokens: Option<u64>,
}
