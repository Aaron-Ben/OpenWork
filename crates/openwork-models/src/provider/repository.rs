use async_trait::async_trait;
use thiserror::Error;

use super::{ProviderInput, ProviderProfile, ProviderRuntimeConfig};

#[derive(Debug, Clone, Error)]
pub enum ProviderRepositoryError {
    #[error("provider not found: {id}")]
    NotFound { id: String },
    #[error("provider already exists: {id}")]
    AlreadyExists { id: String },
    #[error("provider field is invalid: {field}")]
    InvalidInput { field: &'static str },
    #[error(
        "provider {id} has no API key: set apiKey, or set envKey to an environment variable that holds the key"
    )]
    MissingCredential { id: String },
    #[error("provider persistence failed: {message}")]
    Persistence { message: String },
}

#[async_trait]
pub trait ProviderRepository: Send + Sync {
    async fn list_profiles(&self) -> Result<Vec<ProviderProfile>, ProviderRepositoryError>;
    async fn get_profile(
        &self,
        id: &str,
    ) -> Result<Option<ProviderProfile>, ProviderRepositoryError>;
    /// 返回连接配置与已解析的密钥。Provider 存在但没有可用的密钥时，返回 `MissingCredential`。
    async fn load_runtime(
        &self,
        id: &str,
    ) -> Result<Option<ProviderRuntimeConfig>, ProviderRepositoryError>;
    async fn create(
        &self,
        id: &str,
        input: ProviderInput,
    ) -> Result<ProviderProfile, ProviderRepositoryError>;
    async fn update(
        &self,
        id: &str,
        input: ProviderInput,
    ) -> Result<ProviderProfile, ProviderRepositoryError>;
    async fn delete(&self, id: &str) -> Result<(), ProviderRepositoryError>;
}
