use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OpenAiChatDialect {
    Standard,
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

impl ProviderDriver {
    pub fn code(self) -> &'static str {
        match self {
            Self::OpenaiResponses => "openai_responses",
            Self::AnthropicMessages => "anthropic_messages",
            Self::OpenaiChat(OpenAiChatDialect::Standard) => "openai_chat_standard",
            Self::OpenaiChat(OpenAiChatDialect::Deepseek) => "openai_chat_deepseek",
            Self::OpenaiChat(OpenAiChatDialect::Kimi) => "openai_chat_kimi",
            Self::OpenaiChat(OpenAiChatDialect::Qwen) => "openai_chat_qwen",
            Self::OpenaiChat(OpenAiChatDialect::Glm) => "openai_chat_glm",
        }
    }
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
    #[serde(alias = "openai_chat")]
    OpenaiCompatible,
}

impl ProviderKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Openai => "openai",
            Self::Glm => "glm",
            Self::Kimi => "kimi",
            Self::Deepseek => "deepseek",
            Self::Qwen => "qwen",
            Self::Anthropic => "anthropic",
            Self::OpenaiCompatible => "openai_compatible",
        }
    }

    pub fn driver(self) -> ProviderDriver {
        match self {
            Self::Openai => ProviderDriver::OpenaiResponses,
            Self::Anthropic => ProviderDriver::AnthropicMessages,
            Self::OpenaiCompatible => ProviderDriver::OpenaiChat(OpenAiChatDialect::Standard),
            Self::Deepseek => ProviderDriver::OpenaiChat(OpenAiChatDialect::Deepseek),
            Self::Kimi => ProviderDriver::OpenaiChat(OpenAiChatDialect::Kimi),
            Self::Qwen => ProviderDriver::OpenaiChat(OpenAiChatDialect::Qwen),
            Self::Glm => ProviderDriver::OpenaiChat(OpenAiChatDialect::Glm),
        }
    }

    pub fn driver_code(self) -> &'static str {
        self.driver().code()
    }

    pub fn from_driver_code(value: &str) -> Option<Self> {
        match value {
            "openai_responses" => Some(Self::Openai),
            "anthropic_messages" => Some(Self::Anthropic),
            "openai_chat_standard" => Some(Self::OpenaiCompatible),
            "openai_chat_deepseek" => Some(Self::Deepseek),
            "openai_chat_kimi" => Some(Self::Kimi),
            "openai_chat_qwen" => Some(Self::Qwen),
            "openai_chat_glm" => Some(Self::Glm),
            _ => None,
        }
    }
}
