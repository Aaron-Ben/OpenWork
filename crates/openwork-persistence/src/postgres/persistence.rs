use openwork_database::{Database, DatabaseConfig};
use openwork_protocol::provider::ProviderRepositoryError;
use sqlx::PgPool;

use crate::ApiKeyCipher;

use super::{PostgresProviderRepository, migrations::PROVIDER_MIGRATIONS};

/// PostgreSQL composition root：统一拥有连接池和全库 migration 生命周期。
#[derive(Clone)]
pub struct PostgresPersistence {
    database: Database,
    api_key_cipher: ApiKeyCipher,
}

impl PostgresPersistence {
    pub async fn connect(config: DatabaseConfig) -> Result<Self, ProviderRepositoryError> {
        let api_key_cipher = ApiKeyCipher::from_env().map_err(persistence_error)?;
        Self::connect_with_cipher(config, api_key_cipher).await
    }

    pub async fn connect_with_cipher(
        config: DatabaseConfig,
        api_key_cipher: ApiKeyCipher,
    ) -> Result<Self, ProviderRepositoryError> {
        let database = Database::connect(config).await.map_err(persistence_error)?;
        database
            .migrate(PROVIDER_MIGRATIONS)
            .await
            .map_err(persistence_error)?;
        Ok(Self {
            database,
            api_key_cipher,
        })
    }

    pub async fn connect_from_env_or_local() -> Result<Self, ProviderRepositoryError> {
        Self::connect(DatabaseConfig::from_env_or_local()).await
    }

    pub fn pool(&self) -> &PgPool {
        self.database.pool()
    }

    pub fn provider_repository(&self) -> PostgresProviderRepository {
        PostgresProviderRepository::new(self.pool().clone(), self.api_key_cipher.clone())
    }
}

fn persistence_error(error: impl std::fmt::Display) -> ProviderRepositoryError {
    ProviderRepositoryError::Persistence {
        message: error.to_string(),
    }
}
