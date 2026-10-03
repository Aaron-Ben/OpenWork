use serde::{Deserialize, Serialize};

use super::{Message, Role};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelRequest {
    pub model: String,
    pub messages: Vec<Message>,
    pub temperature: Option<f32>,
    #[serde(default)]
    pub top_p: Option<f32>,
    pub max_output_tokens: Option<u32>,
    /// Responses 的 `reasoning.effort`，取值来自模型目录，原样发送。`None` 时不发送 `reasoning`。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_effort: Option<String>,
    #[serde(default)]
    pub tools: Vec<ToolDefinition>,
    /// Responses 的 `prompt_cache_key`：同一 Session 的请求带相同的值，提高提示缓存命中。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt_cache_key: Option<String>,
}

impl ModelRequest {
    pub fn text(model: impl Into<String>, prompt: impl Into<String>) -> Self {
        Self {
            model: model.into(),
            messages: vec![Message::text(Role::User, prompt)],
            temperature: None,
            top_p: None,
            max_output_tokens: None,
            reasoning_effort: None,
            tools: Vec::new(),
            prompt_cache_key: None,
        }
    }

    pub fn with_reasoning_effort(mut self, effort: impl Into<String>) -> Self {
        self.reasoning_effort = Some(effort.into());
        self
    }
}
