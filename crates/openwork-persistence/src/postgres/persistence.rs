use openwork_database::{Database, DatabaseConfig};
use openwork_protocol::provider::ProviderRepositoryError;
use sqlx::PgPool;

use super::{PostgresProviderRepository, migrations::PROVIDER_MIGRATIONS};

/// PostgreSQL composition root：统一拥有连接池和全库 migration 生命周期。
#[derive(Clone)]
pub struct PostgresPersistence {
    database: Database,
}

impl PostgresPersistence {
    pub async fn connect(config: DatabaseConfig) -> Result<Self, ProviderRepositoryError> {
        let database = Database::connect(config).await.map_err(persistence_error)?;
        database
            .migrate(PROVIDER_MIGRATIONS)
            .await
            .map_err(persistence_error)?;
        Ok(Self { database })
    }

    pub async fn connect_from_env_or_local() -> Result<Self, ProviderRepositoryError> {
        Self::connect(DatabaseConfig::from_env_or_local()).await
    }

    pub fn pool(&self) -> &PgPool {
        self.database.pool()
    }

    pub fn provider_repository(&self) -> PostgresProviderRepository {
        PostgresProviderRepository::new(self.pool().clone())
    }
}

fn persistence_error(error: impl std::fmt::Display) -> ProviderRepositoryError {
    ProviderRepositoryError::Persistence {
        message: error.to_string(),
    }
}
