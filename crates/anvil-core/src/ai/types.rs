use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelCapability {
    Chat,
    Reasoning,
    Embedding,
    Vision,
    VideoInput,
    ToolCalling,
    JsonSchema,
    Streaming,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelSpec {
    pub provider: String,
    pub model: String,
    pub capabilities: Vec<ModelCapability>,
}

impl ModelSpec {
    pub fn supports(&self, capability: ModelCapability) -> bool {
        self.capabilities.contains(&capability)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    System,
    User,
    Assistant,
    Tool,
}

impl Role {
    pub fn as_provider_str(self) -> &'static str {
        match self {
            Role::System => "system",
            Role::User => "user",
            Role::Assistant => "assistant",
            Role::Tool => "tool",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Message {
    pub role: Role,
    pub content: Vec<ContentBlock>,
}

impl Message {
    pub fn text(role: Role, content: impl Into<String>) -> Self {
        Self {
            role,
            content: vec![ContentBlock::text(content)],
        }
    }

    pub fn assistant_with_thinking(text: impl Into<String>, thinking: impl Into<String>) -> Self {
        Self {
            role: Role::Assistant,
            content: vec![ContentBlock::thinking(thinking), ContentBlock::text(text)],
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentBlock {
    Text(TextBlock),
    Thinking(ThinkingBlock),
    Data(DataBlock),
    ToolCall(ToolCallBlock),
    ToolResult(ToolResultBlock),
}

impl ContentBlock {
    pub fn text(text: impl Into<String>) -> Self {
        Self::Text(TextBlock { text: text.into() })
    }

    pub fn thinking(thinking: impl Into<String>) -> Self {
        Self::Thinking(ThinkingBlock {
            thinking: thinking.into(),
        })
    }

    pub fn image_url(url: impl Into<String>, media_type: impl Into<String>) -> Self {
        Self::Data(DataBlock {
            source: DataSource::Url {
                url: url.into(),
                media_type: media_type.into(),
            },
            name: None,
        })
    }

    pub fn image_base64(
        data: impl Into<String>,
        media_type: impl Into<String>,
        name: Option<String>,
    ) -> Self {
        Self::Data(DataBlock {
            source: DataSource::Base64(Base64Source {
                data: data.into(),
                media_type: media_type.into(),
            }),
            name,
        })
    }

    pub fn file_id(id: impl Into<String>) -> Self {
        Self::Data(DataBlock {
            source: DataSource::FileId { id: id.into() },
            name: None,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TextBlock {
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ThinkingBlock {
    pub thinking: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DataBlock {
    pub source: DataSource,
    pub name: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "source_type", rename_all = "snake_case")]
pub enum DataSource {
    Url { url: String, media_type: String },
    Base64(Base64Source),
    FileId { id: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Base64Source {
    pub data: String,
    pub media_type: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolCallBlock {
    pub id: String,
    pub name: String,
    pub input: String,
    pub state: ToolCallState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolCallState {
    Pending,
    Submitted,
    Finished,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolResultBlock {
    pub id: String,
    pub name: String,
    pub output: Vec<ContentBlock>,
    pub state: ToolResultState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolResultState {
    Success,
    Error,
    Interrupted,
    Denied,
    Running,
}

/// 声明给模型的工具定义(对应 OpenAI `tools[].function`)。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    /// JSON Schema,描述工具参数结构。
    pub parameters: serde_json::Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GenerateRequest {
    pub model: String,
    pub messages: Vec<Message>,
    pub temperature: Option<f32>,
    pub max_tokens: Option<u32>,
    pub stream: bool,
    pub thinking: Option<ThinkingConfig>,
    /// 可用工具声明。OpenAI-compatible chat 走 function calling;
    /// 未适配 tool calling 的 provider 在非空时应返回 `InvalidRequest`。
    #[serde(default)]
    pub tools: Vec<ToolDefinition>,
}

impl GenerateRequest {
    pub fn text(model: impl Into<String>, prompt: impl Into<String>) -> Self {
        Self {
            model: model.into(),
            messages: vec![Message::text(Role::User, prompt)],
            temperature: None,
            max_tokens: None,
            stream: false,
            thinking: None,
            tools: Vec::new(),
        }
    }

    pub fn with_thinking(mut self, thinking: ThinkingConfig) -> Self {
        self.thinking = Some(thinking);
        self
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GenerateResponse {
    pub text: String,
    pub reasoning_text: Option<String>,
    pub usage: Option<TokenUsage>,
    pub raw: serde_json::Value,
    /// 模型本轮请求的工具调用(按到达顺序)。流式与非流式路径都应填充。
    #[serde(default)]
    pub tool_calls: Vec<ToolCallBlock>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum GenerateStreamEvent {
    TextDelta { delta: String },
    ReasoningDelta { delta: String },
    /// 工具调用开始:模型给出了工具 `id` 与 `name`(参数可能随后以增量到达)。
    ToolCallStart { id: String, name: String },
    /// 工具调用参数增量:一段 JSON 片段,需按 `id` 拼接成完整参数字符串。
    ToolCallDelta { id: String, partial_input: String },
    /// 工具调用结束:`id` 的参数已全部到达。
    ToolCallEnd { id: String },
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EmbeddingRequest {
    pub model: String,
    pub input: Vec<String>,
    pub dimensions: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EmbeddingResponse {
    pub vectors: Vec<Vec<f32>>,
    pub usage: Option<TokenUsage>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TokenUsage {
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub total_tokens: Option<u64>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_spec_checks_capability() {
        let spec = ModelSpec {
            provider: "openai".to_string(),
            model: "gpt-4.1".to_string(),
            capabilities: vec![ModelCapability::Chat, ModelCapability::Vision],
        };

        assert!(spec.supports(ModelCapability::Chat));
        assert!(!spec.supports(ModelCapability::Embedding));
    }

    #[test]
    fn message_can_store_thinking_as_content_block() {
        let message = Message::assistant_with_thinking("answer", "reasoning");

        assert!(matches!(message.content[0], ContentBlock::Thinking(_)));
        assert!(matches!(message.content[1], ContentBlock::Text(_)));
    }
}
