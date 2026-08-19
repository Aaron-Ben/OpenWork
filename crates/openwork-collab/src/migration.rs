use sqlx::{Executor, PgPool};
use thiserror::Error;

const P1_VERSION: i64 = 202_608_180_001;
const P1_DESCRIPTION: &str = "collab p1";
const P1_SQL: &str = include_str!("../migrations/202608180001_collab_p1.sql");

pub async fn migrate(pool: &PgPool) -> Result<(), MigrationError> {
    let mut transaction = pool.begin().await?;
    transaction
        .execute(
            "CREATE TABLE IF NOT EXISTS collab_schema_migrations (
                version      BIGINT PRIMARY KEY,
                description  TEXT NOT NULL,
                installed_at TIMESTAMP WITHOUT TIME ZONE NOT NULL
                             DEFAULT (CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai')
             )",
        )
        .await?;
    let applied: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM collab_schema_migrations WHERE version = $1)",
    )
    .bind(P1_VERSION)
    .fetch_one(&mut *transaction)
    .await?;
    if !applied {
        sqlx::raw_sql(P1_SQL).execute(&mut *transaction).await?;
        sqlx::query("INSERT INTO collab_schema_migrations (version, description) VALUES ($1, $2)")
            .bind(P1_VERSION)
            .bind(P1_DESCRIPTION)
            .execute(&mut *transaction)
            .await?;
    }
    transaction.commit().await?;
    Ok(())
}

#[derive(Debug, Error)]
pub enum MigrationError {
    #[error("collaboration migration failed: {0}")]
    Database(#[from] sqlx::Error),
}

#[cfg(test)]
mod tests {
    use super::P1_SQL;

    #[test]
    fn p1_schema_obeys_time_and_terminal_run_constraints() {
        assert!(!P1_SQL.contains("TIMESTAMP WITH TIME ZONE"));
        assert!(!P1_SQL.contains("TIMESTAMPTZ"));
        assert!(P1_SQL.contains("AT TIME ZONE 'Asia/Shanghai'"));
        assert!(P1_SQL.contains("collab_runs_terminal_time_valid"));
        assert!(P1_SQL.contains("status =  'running' AND ended_at IS     NULL"));
        assert!(P1_SQL.contains("status <> 'running' AND ended_at IS NOT NULL"));
    }
}
