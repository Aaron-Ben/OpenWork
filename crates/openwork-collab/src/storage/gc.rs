use time::Duration;

use crate::{storage::StorageError, time::china_now};

use super::CollabStorage;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CollabGcPolicy {
    event_retention_days: u32,
    triage_retention_days: u32,
    batch_size: u32,
    statement_timeout_ms: u32,
}

impl CollabGcPolicy {
    pub fn new(
        event_retention_days: u32,
        triage_retention_days: u32,
        batch_size: u32,
        statement_timeout_ms: u32,
    ) -> Result<Self, StorageError> {
        if event_retention_days == 0 || triage_retention_days == 0 {
            return Err(StorageError::InvalidInput(
                "collaboration retention days must be positive".to_string(),
            ));
        }
        if batch_size == 0 {
            return Err(StorageError::InvalidInput(
                "collaboration GC batch size must be positive".to_string(),
            ));
        }
        if statement_timeout_ms == 0 {
            return Err(StorageError::InvalidInput(
                "collaboration GC statement timeout must be positive".to_string(),
            ));
        }
        Ok(Self {
            event_retention_days,
            triage_retention_days,
            batch_size,
            statement_timeout_ms,
        })
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CollabGcOutcome {
    pub events_deleted: u64,
    pub triages_deleted: u64,
}

impl CollabGcOutcome {
    fn add(&mut self, other: Self) {
        self.events_deleted = self.events_deleted.saturating_add(other.events_deleted);
        self.triages_deleted = self.triages_deleted.saturating_add(other.triages_deleted);
    }
}

impl CollabStorage {
    pub async fn gc_batch(&self, policy: CollabGcPolicy) -> Result<CollabGcOutcome, StorageError> {
        let event_cutoff = china_now() - Duration::days(i64::from(policy.event_retention_days));
        let triage_cutoff = china_now() - Duration::days(i64::from(policy.triage_retention_days));
        Ok(CollabGcOutcome {
            events_deleted: self
                .delete_observation_batch(
                    "collab_events",
                    event_cutoff,
                    policy.batch_size,
                    policy.statement_timeout_ms,
                )
                .await?,
            triages_deleted: self
                .delete_observation_batch(
                    "collab_triages",
                    triage_cutoff,
                    policy.batch_size,
                    policy.statement_timeout_ms,
                )
                .await?,
        })
    }

    pub async fn garbage_collect(
        &self,
        policy: CollabGcPolicy,
    ) -> Result<CollabGcOutcome, StorageError> {
        let mut total = CollabGcOutcome::default();
        loop {
            let batch = self.gc_batch(policy).await?;
            total.add(batch);
            if batch.events_deleted < u64::from(policy.batch_size)
                && batch.triages_deleted < u64::from(policy.batch_size)
            {
                return Ok(total);
            }
            tokio::task::yield_now().await;
        }
    }

    async fn delete_observation_batch(
        &self,
        table: &str,
        cutoff: time::PrimitiveDateTime,
        batch_size: u32,
        statement_timeout_ms: u32,
    ) -> Result<u64, StorageError> {
        let mut transaction = self.pool().begin().await?;
        sqlx::query("SELECT set_config('statement_timeout', $1, TRUE)")
            .bind(format!("{statement_timeout_ms}ms"))
            .execute(&mut *transaction)
            .await?;
        let sql = format!(
            "DELETE FROM {table}
              WHERE ctid IN (
                    SELECT ctid FROM {table}
                     WHERE created_at < $1
                     ORDER BY created_at, id
                     LIMIT $2
              )"
        );
        let result = sqlx::query(&sql)
            .bind(cutoff)
            .bind(i64::from(batch_size))
            .execute(&mut *transaction)
            .await?;
        transaction.commit().await?;
        Ok(result.rows_affected())
    }
}
