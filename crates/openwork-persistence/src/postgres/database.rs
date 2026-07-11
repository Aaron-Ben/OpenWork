use std::time::Duration;

use sqlx::{Executor, PgPool, postgres::PgPoolOptions};

use super::migrations::{Migration, run_migrations};

const DEFAULT_DATABASE_URL: &str = "postgres://openwork:openwork@localhost:5432/openwork";

#[derive(Debug, Clone)]
pub struct DatabaseConfig {
    pub url: String,
    pub max_connections: u32,
    pub acquire_timeout: Duration,
}

impl DatabaseConfig {
    pub fn from_env_or_local() -> Self {
        Self {
            url: std::env::var("DATABASE_URL").unwrap_or_else(|_| DEFAULT_DATABASE_URL.to_string()),
            max_connections: 8,
            acquire_timeout: Duration::from_secs(10),
        }
    }
}

#[derive(Clone)]
pub(super) struct Database {
    pool: PgPool,
}

impl Database {
    pub async fn connect(config: DatabaseConfig) -> Result<Self, sqlx::Error> {
        let pool = PgPoolOptions::new()
            .max_connections(config.max_connections)
            .acquire_timeout(config.acquire_timeout)
            .after_connect(|connection, _metadata| {
                Box::pin(async move {
                    connection.execute("SET TIME ZONE 'Asia/Shanghai'").await?;
                    Ok(())
                })
            })
            .connect(&config.url)
            .await?;
        Ok(Self { pool })
    }

    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    pub async fn migrate(&self, migrations: &[Migration]) -> Result<(), sqlx::Error> {
        run_migrations(&self.pool, migrations).await
    }
}
