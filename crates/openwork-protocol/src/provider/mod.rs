//! Provider 配置领域类型与持久化 Port。

mod driver;
mod profile;
mod repository;

pub use driver::{OpenAiChatDialect, ProviderDriver, ProviderKind};
pub use profile::{
    ApiCredential, ModelTier, ProviderInput, ProviderModel, ProviderProfile, ProviderRuntimeConfig,
};
pub use repository::{ProviderRepository, ProviderRepositoryError};
