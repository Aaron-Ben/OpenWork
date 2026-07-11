use std::time::Duration;

use openwork_persistence::DatabaseConfig;
use sqlx::{Executor, PgPool, postgres::PgPoolOptions};

pub fn test_config(max_connections: u32) -> Option<DatabaseConfig> {
    Some(DatabaseConfig {
        url: std::env::var("TEST_DATABASE_URL").ok()?,
        max_connections,
        acquire_timeout: Duration::from_secs(5),
    })
}

pub async fn connect_test_pool(config: &DatabaseConfig) -> Result<PgPool, sqlx::Error> {
    PgPoolOptions::new()
        .max_connections(config.max_connections)
        .acquire_timeout(config.acquire_timeout)
        .after_connect(|connection, _metadata| {
            Box::pin(async move {
                connection.execute("SET TIME ZONE 'Asia/Shanghai'").await?;
                Ok(())
            })
        })
        .connect(&config.url)
        .await
}
