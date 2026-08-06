use std::collections::BTreeSet;

use super::{PostgresStorage, StorageError};

impl PostgresStorage {
    pub async fn disabled_skill_names(&self) -> Result<BTreeSet<String>, StorageError> {
        let names = sqlx::query_scalar::<_, String>(
            "SELECT name FROM skill_status WHERE disabled = TRUE ORDER BY name",
        )
        .fetch_all(self.pool())
        .await?;
        Ok(names.into_iter().collect())
    }

    pub async fn set_skill_disabled(&self, name: &str, disabled: bool) -> Result<(), StorageError> {
        sqlx::query(
            "INSERT INTO skill_status (name, disabled)
             VALUES ($1, $2)
             ON CONFLICT (name) DO UPDATE SET disabled = EXCLUDED.disabled",
        )
        .bind(name)
        .bind(disabled)
        .execute(self.pool())
        .await?;
        Ok(())
    }
}
