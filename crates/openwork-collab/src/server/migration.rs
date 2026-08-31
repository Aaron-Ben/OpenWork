use sqlx::{Executor, PgPool};

const R3_VERSION: i64 = 202_608_310_001;
const R3_SQL: &str = include_str!("../../migrations/202608310001_collab_r3.sql");

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
    .bind(R3_VERSION)
    .fetch_one(&mut *transaction)
    .await?;
    if !applied {
        sqlx::raw_sql(R3_SQL).execute(&mut *transaction).await?;
        sqlx::query(
            "INSERT INTO collab_schema_migrations (version, description)
             VALUES ($1, 'collaboration r3 atomic cutover')",
        )
        .bind(R3_VERSION)
        .execute(&mut *transaction)
        .await?;
    }
    transaction.commit().await
}
