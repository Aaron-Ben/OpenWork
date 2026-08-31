use sqlx::{FromRow, PgPool};

use crate::protocol::{EngineInventoryView, EngineObservation, EngineStatus};

#[derive(Clone)]
pub(crate) struct EngineInventory {
    pool: PgPool,
}

impl EngineInventory {
    pub(crate) fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub(crate) async fn report(
        &self,
        runtime_session_id: &str,
        observations: &[EngineObservation],
    ) -> Result<(), sqlx::Error> {
        let mut transaction = self.pool.begin().await?;
        for observation in observations {
            if observation.engine_id.trim().is_empty() {
                return Err(sqlx::Error::Protocol(
                    "INVALID_ARGUMENT: Engine id cannot be empty".to_string(),
                ));
            }
            let status = status_name(observation.status);
            sqlx::query(
                "INSERT INTO collab_engine_inventory (
                    engine_id, status, version, checked_at, last_error, observed_session_id
                 ) VALUES (
                    $1, $2, $3, to_timestamp($4) AT TIME ZONE 'UTC', $5, $6
                 )
                 ON CONFLICT (engine_id) DO UPDATE SET
                    status = EXCLUDED.status,
                    version = EXCLUDED.version,
                    checked_at = EXCLUDED.checked_at,
                    last_error = EXCLUDED.last_error,
                    observed_session_id = EXCLUDED.observed_session_id",
            )
            .bind(observation.engine_id.trim())
            .bind(status)
            .bind(&observation.version)
            .bind(observation.checked_at)
            .bind(&observation.last_error)
            .bind(runtime_session_id)
            .execute(&mut *transaction)
            .await?;
        }
        transaction.commit().await
    }

    pub(crate) async fn list(&self) -> Result<Vec<EngineInventoryView>, sqlx::Error> {
        sqlx::query_as::<_, InventoryRow>(
            "SELECT engine_id, status, version,
                    floor(extract(epoch FROM checked_at))::BIGINT AS checked_at,
                    last_error, observed_session_id
             FROM collab_engine_inventory ORDER BY engine_id",
        )
        .fetch_all(&self.pool)
        .await
        .map(|rows| rows.into_iter().map(EngineInventoryView::from).collect())
    }
}

#[derive(FromRow)]
struct InventoryRow {
    engine_id: String,
    status: String,
    version: Option<String>,
    checked_at: i64,
    last_error: Option<String>,
    observed_session_id: String,
}

impl From<InventoryRow> for EngineInventoryView {
    fn from(row: InventoryRow) -> Self {
        Self {
            engine_id: row.engine_id,
            status: match row.status.as_str() {
                "ready" => EngineStatus::Ready,
                "missing" => EngineStatus::Missing,
                "error" => EngineStatus::Error,
                _ => EngineStatus::Unknown,
            },
            version: row.version,
            checked_at: row.checked_at,
            last_error: row.last_error,
            observed_session_id: row.observed_session_id,
        }
    }
}

fn status_name(status: EngineStatus) -> &'static str {
    match status {
        EngineStatus::Unknown => "unknown",
        EngineStatus::Ready => "ready",
        EngineStatus::Missing => "missing",
        EngineStatus::Error => "error",
    }
}
