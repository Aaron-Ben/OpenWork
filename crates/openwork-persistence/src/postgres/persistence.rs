use std::sync::Arc;

use sqlx::PgPool;
use thiserror::Error;

use crate::{ApiKeyCipher, SessionStore};

use super::{
    PostgresEventJournal, PostgresProviderRepository,
    database::{Database, DatabaseConfig},
    migrations::{
        DATABASE_INFRA_MIGRATIONS, DROP_LEGACY_SESSION_MIGRATIONS, PROVIDER_MIGRATIONS,
        RECORDED_EVENT_MIGRATIONS,
    },
};

const REQUIRED_TABLES: &[&str] = &[
    "schema_migrations",
    "providers",
    "provider_models",
    "recorded_events",
];
#[derive(Debug, Error)]
pub enum PostgresPersistenceError {
    #[error("database error: {message}")]
    Database { message: String },
    #[error("API key encryption configuration error: {message}")]
    Encryption { message: String },
    #[error(
        "database schema is not ready: {details}. run `cargo run -p openwork-persistence --bin openwork-migrate`"
    )]
    SchemaNotReady { details: String },
}

/// PostgreSQL composition root：统一拥有连接池和全库 migration 生命周期。
#[derive(Clone)]
pub struct PostgresPersistence {
    database: Database,
    api_key_cipher: ApiKeyCipher,
}

impl PostgresPersistence {
    /// Connects to an already migrated database. It never creates tables.
    pub async fn connect(config: DatabaseConfig) -> Result<Self, PostgresPersistenceError> {
        let api_key_cipher = ApiKeyCipher::from_env().map_err(encryption_error)?;
        Self::connect_with_cipher(config, api_key_cipher).await
    }

    pub async fn connect_with_cipher(
        config: DatabaseConfig,
        api_key_cipher: ApiKeyCipher,
    ) -> Result<Self, PostgresPersistenceError> {
        let database = Database::connect(config).await.map_err(database_error)?;
        let persistence = Self {
            database,
            api_key_cipher,
        };
        persistence.verify_schema().await?;
        Ok(persistence)
    }

    pub async fn connect_from_env_or_local() -> Result<Self, PostgresPersistenceError> {
        Self::connect(DatabaseConfig::from_env_or_local()).await
    }

    /// Explicit schema lifecycle entry point used by the migration binary.
    /// It deliberately does not require the API-key master key.
    pub async fn migrate_database(config: DatabaseConfig) -> Result<(), PostgresPersistenceError> {
        let database = Database::connect(config).await.map_err(database_error)?;
        for migrations in [
            PROVIDER_MIGRATIONS,
            RECORDED_EVENT_MIGRATIONS,
            DROP_LEGACY_SESSION_MIGRATIONS,
            DATABASE_INFRA_MIGRATIONS,
        ] {
            database.migrate(migrations).await.map_err(database_error)?;
        }
        Ok(())
    }

    pub async fn migrate_from_env_or_local() -> Result<(), PostgresPersistenceError> {
        Self::migrate_database(DatabaseConfig::from_env_or_local()).await
    }

    pub fn pool(&self) -> &PgPool {
        self.database.pool()
    }

    pub fn provider_repository(&self) -> PostgresProviderRepository {
        PostgresProviderRepository::new(self.pool().clone(), self.api_key_cipher.clone())
    }

    pub fn event_journal(&self) -> PostgresEventJournal {
        PostgresEventJournal::new(self.pool().clone())
    }

    pub fn session_store(&self) -> SessionStore {
        SessionStore::new(Arc::new(self.event_journal()))
    }

    async fn verify_schema(&self) -> Result<(), PostgresPersistenceError> {
        let mut missing = Vec::new();
        for table in REQUIRED_TABLES {
            let exists: bool =
                sqlx::query_scalar("SELECT to_regclass('public.' || $1) IS NOT NULL")
                    .bind(table)
                    .fetch_one(self.pool())
                    .await
                    .map_err(database_error)?;
            if !exists {
                missing.push(*table);
            }
        }
        if !missing.is_empty() {
            Err(PostgresPersistenceError::SchemaNotReady {
                details: format!("missing tables: {}", missing.join(", ")),
            })
        } else {
            let required_migration = RECORDED_EVENT_MIGRATIONS
                .last()
                .expect("recorded event migrations must not be empty");
            let migration_applied: bool = sqlx::query_scalar(
                "SELECT EXISTS(
                   SELECT 1 FROM schema_migrations WHERE version = $1
                 )",
            )
            .bind(required_migration.version)
            .fetch_one(self.pool())
            .await
            .map_err(database_error)?;
            if migration_applied {
                Ok(())
            } else {
                Err(PostgresPersistenceError::SchemaNotReady {
                    details: format!(
                        "required migration {} ({}) has not been applied",
                        required_migration.version, required_migration.name
                    ),
                })
            }
        }
    }
}

fn database_error(error: impl std::fmt::Display) -> PostgresPersistenceError {
    PostgresPersistenceError::Database {
        message: error.to_string(),
    }
}

fn encryption_error(error: impl std::fmt::Display) -> PostgresPersistenceError {
    PostgresPersistenceError::Encryption {
        message: error.to_string(),
    }
}
