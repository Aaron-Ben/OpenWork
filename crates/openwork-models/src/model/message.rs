use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::provider::ProviderDriver;

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
    ProviderOpaque(ProviderOpaqueBlock),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProviderOpaqueBlock {
    pub driver: ProviderDriver,
    pub kind: String,
    pub payload: Value,
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
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub artifacts: Vec<ToolResultArtifact>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolResultArtifact {
    pub kind: String,
    pub payload: Value,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn message_can_store_thinking_as_content_block() {
        let message = Message::assistant_with_thinking("answer", "reasoning");
        assert!(matches!(message.content[0], ContentBlock::Thinking(_)));
        assert!(matches!(message.content[1], ContentBlock::Text(_)));
    }

    #[test]
    fn tool_result_artifacts_are_optional_and_round_trip() {
        let legacy = serde_json::json!({
            "id": "call-1",
            "name": "write",
            "output": [{"type": "text", "text": "created file"}],
            "state": "success"
        });
        let legacy: ToolResultBlock = serde_json::from_value(legacy).expect("legacy result");
        assert!(legacy.artifacts.is_empty());

        let result = ToolResultBlock {
            id: "call-2".to_string(),
            name: "write".to_string(),
            output: vec![ContentBlock::text("edited file")],
            state: ToolResultState::Success,
            artifacts: vec![ToolResultArtifact {
                kind: "file_change".to_string(),
                payload: serde_json::json!({"changeId": "change-2"}),
            }],
        };
        let encoded = serde_json::to_value(&result).expect("serialize result");
        let decoded: ToolResultBlock = serde_json::from_value(encoded).expect("deserialize result");

        assert_eq!(decoded.artifacts, result.artifacts);
    }
}
