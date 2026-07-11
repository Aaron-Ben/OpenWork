use sqlx::PgPool;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Migration {
    pub version: i64,
    pub name: &'static str,
    pub statements: &'static [&'static str],
}

pub(crate) async fn run_migrations(
    pool: &PgPool,
    migrations: &[Migration],
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS schema_migrations (
           version BIGINT PRIMARY KEY,
           name TEXT NOT NULL,
           applied_at TIMESTAMP WITHOUT TIME ZONE NOT NULL
               DEFAULT (CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai')
         )",
    )
    .execute(pool)
    .await?;

    for migration in migrations {
        let exists: Option<i64> =
            sqlx::query_scalar("SELECT 1::BIGINT FROM schema_migrations WHERE version = $1")
                .bind(migration.version)
                .fetch_optional(pool)
                .await?;
        if exists.is_some() {
            continue;
        }

        let mut transaction = pool.begin().await?;
        for statement in migration.statements {
            sqlx::query(statement).execute(&mut *transaction).await?;
        }
        sqlx::query(
            "INSERT INTO schema_migrations (version, name)
             VALUES ($1, $2)",
        )
        .bind(migration.version)
        .bind(migration.name)
        .execute(&mut *transaction)
        .await?;
        transaction.commit().await?;
    }

    Ok(())
}
