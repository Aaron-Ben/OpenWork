//! Model contracts, provider profiles, protocol adapters, and transport.

pub mod model;
pub mod provider;

mod adapters;
mod factory;
mod gateway;
mod transport;

pub(crate) use adapters::error;
pub(crate) use transport::http as config;
pub(crate) use transport::sse;

pub(crate) use adapters::OpenAiCompatibleChatProvider;
pub use adapters::{
    AnthropicProvider, DeepSeekProvider, GlmProvider, KimiProvider, OpenAiProvider, QwenProvider,
};
pub use factory::ProviderFactory;
pub use gateway::{RetryDecision, RetryPolicy, RetryingModelPort};
pub use model::*;
pub use provider::*;
pub use transport::{HttpProviderConfig, HttpTransport, SseFrame, SseFramer};
