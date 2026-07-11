mod adapters;
mod factory;
mod gateway;
mod transport;

// Crate 内部兼容别名；gateway/transport 的公开归属不受目录重构影响。
pub(crate) use adapters::error;
pub(crate) use transport::http as config;
pub(crate) use transport::sse;

pub(crate) use adapters::OpenAiCompatibleChatProvider;
pub use adapters::{
    AnthropicProvider, DeepSeekProvider, GlmProvider, KimiProvider, OpenAiProvider, QwenProvider,
};
pub use factory::ProviderFactory;
pub use gateway::{RetryDecision, RetryPolicy, RetryingModelPort};
pub use transport::{HttpProviderConfig, HttpTransport, SseFrame, SseFramer};
