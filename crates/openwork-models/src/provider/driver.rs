use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OpenAiChatDialect {
    Deepseek,
    Kimi,
    Qwen,
    Glm,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderDriver {
    OpenaiResponses,
    AnthropicMessages,
    OpenaiChat(OpenAiChatDialect),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderKind {
    #[serde(alias = "openai_responses")]
    Openai,
    Glm,
    Kimi,
    Deepseek,
    Qwen,
    Anthropic,
}

impl ProviderKind {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "openai" => Some(Self::Openai),
            "glm" => Some(Self::Glm),
            "kimi" => Some(Self::Kimi),
            "deepseek" => Some(Self::Deepseek),
            "qwen" => Some(Self::Qwen),
            "anthropic" => Some(Self::Anthropic),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Openai => "openai",
            Self::Glm => "glm",
            Self::Kimi => "kimi",
            Self::Deepseek => "deepseek",
            Self::Qwen => "qwen",
            Self::Anthropic => "anthropic",
        }
    }

    pub fn driver(self) -> ProviderDriver {
        match self {
            Self::Openai => ProviderDriver::OpenaiResponses,
            Self::Anthropic => ProviderDriver::AnthropicMessages,
            Self::Deepseek => ProviderDriver::OpenaiChat(OpenAiChatDialect::Deepseek),
            Self::Kimi => ProviderDriver::OpenaiChat(OpenAiChatDialect::Kimi),
            Self::Qwen => ProviderDriver::OpenaiChat(OpenAiChatDialect::Qwen),
            Self::Glm => ProviderDriver::OpenaiChat(OpenAiChatDialect::Glm),
        }
    }
}
