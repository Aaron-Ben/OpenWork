use async_trait::async_trait;
use thiserror::Error;

use super::{ProviderInput, ProviderProfile, ProviderRuntimeConfig};

#[derive(Debug, Clone, Error)]
pub enum ProviderRepositoryError {
    #[error("provider not found: {id}")]
    NotFound { id: String },
    #[error("cannot delete the active provider: {id}")]
    CannotDeleteActive { id: String },
    #[error("provider field is invalid: {field}")]
    InvalidInput { field: &'static str },
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
    async fn load_runtime(
        &self,
        id: &str,
    ) -> Result<Option<ProviderRuntimeConfig>, ProviderRepositoryError>;
    async fn active_id(&self) -> Result<Option<String>, ProviderRepositoryError>;
    async fn create(
        &self,
        input: ProviderInput,
    ) -> Result<ProviderProfile, ProviderRepositoryError>;
    async fn update(
        &self,
        id: &str,
        input: ProviderInput,
    ) -> Result<ProviderProfile, ProviderRepositoryError>;
    async fn delete(&self, id: &str) -> Result<(), ProviderRepositoryError>;
    async fn activate(&self, id: &str) -> Result<(), ProviderRepositoryError>;
    async fn clear_active(&self) -> Result<(), ProviderRepositoryError>;
}
