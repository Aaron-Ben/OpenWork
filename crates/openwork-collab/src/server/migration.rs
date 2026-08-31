use sqlx::{Executor, PgPool};

const MIGRATIONS: &[(i64, &str, &str)] = &[
    (
        202_608_310_001,
        "collaboration schema",
        include_str!("../../migrations/202608310001_collaboration.sql"),
    ),
    (
        202_609_010_001,
        "run observability",
        include_str!("../../migrations/202609010001_run_observability.sql"),
    ),
];

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
    for &(version, description, sql) in MIGRATIONS {
        let applied: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM collab_schema_migrations WHERE version = $1)",
        )
        .bind(version)
        .fetch_one(&mut *transaction)
        .await?;
        if applied {
            continue;
        }
        sqlx::raw_sql(sql).execute(&mut *transaction).await?;
        sqlx::query("INSERT INTO collab_schema_migrations (version, description) VALUES ($1, $2)")
            .bind(version)
            .bind(description)
            .execute(&mut *transaction)
            .await?;
    }
    transaction.commit().await
}
