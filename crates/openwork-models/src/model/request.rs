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
    pub max_output_tokens: Option<u32>,
    pub thinking: Option<ThinkingConfig>,
    #[serde(default)]
    pub tools: Vec<ToolDefinition>,
}

impl ModelRequest {
    pub fn text(model: impl Into<String>, prompt: impl Into<String>) -> Self {
        Self {
            model: model.into(),
            messages: vec![Message::text(Role::User, prompt)],
            temperature: None,
            max_output_tokens: None,
            thinking: None,
            tools: Vec::new(),
        }
    }

    pub fn with_thinking(mut self, thinking: ThinkingConfig) -> Self {
        self.thinking = Some(thinking);
        self
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ThinkingConfig {
    pub mode: ThinkingMode,
}

impl ThinkingConfig {
    pub fn enabled() -> Self {
        Self {
            mode: ThinkingMode::Enabled,
        }
    }
    pub fn disabled() -> Self {
        Self {
            mode: ThinkingMode::Disabled,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ThinkingMode {
    Enabled,
    Disabled,
}
