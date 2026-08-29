use sqlx::{Executor, PgPool};

const P0_VERSION: i64 = 202_608_300_001;
const P0_SQL: &str = include_str!("../../migrations/202608300001_collab_p0.sql");

pub async fn migrate(pool: &PgPool) -> Result<(), sqlx::Error> {
    let mut transaction = pool.begin().await?;
    transaction
        .execute(
            "CREATE TABLE IF NOT EXISTS collab_schema_migrations (
                version BIGINT PRIMARY KEY,
                description TEXT NOT NULL,
                installed_at TIMESTAMP WITHOUT TIME ZONE NOT NULL
                    DEFAULT (CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai')
            )",
        )
        .await?;
    let applied: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM collab_schema_migrations WHERE version = $1)",
    )
    .bind(P0_VERSION)
    .fetch_one(&mut *transaction)
    .await?;
    if !applied {
        sqlx::raw_sql(P0_SQL).execute(&mut *transaction).await?;
        sqlx::query(
            "INSERT INTO collab_schema_migrations (version, description)
             VALUES ($1, 'collaboration p0 baseline')",
        )
        .bind(P0_VERSION)
        .execute(&mut *transaction)
        .await?;
    }
    transaction.commit().await
}
