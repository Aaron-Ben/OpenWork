use sqlx::{Executor, PgPool};
use thiserror::Error;

const P1_VERSION: i64 = 202_608_180_001;
const P1_DESCRIPTION: &str = "collab p1";
const P1_SQL: &str = include_str!("../migrations/202608180001_collab_p1.sql");
const P3_VERSION: i64 = 202_608_190_001;
const P3_DESCRIPTION: &str = "collab p3";
const P3_SQL: &str = include_str!("../migrations/202608190001_collab_p3.sql");
const P4_VERSION: i64 = 202_608_190_002;
const P4_DESCRIPTION: &str = "collab p4";
const P4_SQL: &str = include_str!("../migrations/202608190002_collab_p4.sql");

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
    for (version, description, sql) in [
        (P1_VERSION, P1_DESCRIPTION, P1_SQL),
        (P3_VERSION, P3_DESCRIPTION, P3_SQL),
        (P4_VERSION, P4_DESCRIPTION, P4_SQL),
    ] {
        let applied: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM collab_schema_migrations WHERE version = $1)",
        )
        .bind(version)
        .fetch_one(&mut *transaction)
        .await?;
        if !applied {
            sqlx::raw_sql(sql).execute(&mut *transaction).await?;
            sqlx::query(
                "INSERT INTO collab_schema_migrations (version, description) VALUES ($1, $2)",
            )
            .bind(version)
            .bind(description)
            .execute(&mut *transaction)
            .await?;
        }
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
    use super::{P1_SQL, P3_SQL, P4_SQL};

    #[test]
    fn p1_schema_obeys_time_and_terminal_run_constraints() {
        assert!(!P1_SQL.contains("TIMESTAMP WITH TIME ZONE"));
        assert!(!P1_SQL.contains("TIMESTAMPTZ"));
        assert!(P1_SQL.contains("AT TIME ZONE 'Asia/Shanghai'"));
        assert!(P1_SQL.contains("collab_runs_terminal_time_valid"));
        assert!(P1_SQL.contains("status =  'running' AND ended_at IS     NULL"));
        assert!(P1_SQL.contains("status <> 'running' AND ended_at IS NOT NULL"));
    }

    #[test]
    fn p3_schema_obeys_time_and_duration_constraints() {
        assert!(!P3_SQL.contains("TIMESTAMP WITH TIME ZONE"));
        assert!(!P3_SQL.contains("TIMESTAMPTZ"));
        assert!(P3_SQL.contains("AT TIME ZONE 'Asia/Shanghai'"));
        assert!(P3_SQL.contains("latency_ms    BIGINT"));
    }

    #[test]
    fn p4_schema_keeps_completion_explicit_and_time_local() {
        assert!(!P4_SQL.contains("TIMESTAMP WITH TIME ZONE"));
        assert!(!P4_SQL.contains("TIMESTAMPTZ"));
        assert!(P4_SQL.contains("AT TIME ZONE 'Asia/Shanghai'"));
        assert!(P4_SQL.contains("is_done  BOOLEAN NOT NULL DEFAULT FALSE"));
        assert!(P4_SQL.contains("collab_cards_claim_consistent"));
    }
}
