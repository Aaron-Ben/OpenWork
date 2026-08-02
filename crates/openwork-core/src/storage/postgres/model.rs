use super::*;

impl PostgresStorage {
    pub async fn upsert_model(&self, input: &ModelInput) -> Result<(), StorageError> {
        validate_model(input)?;
        sqlx::query(
            "INSERT INTO models (
                 id, display_name, provider_kind, model_name, base_url,
                 credential_ref, enabled, config
             ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
             ON CONFLICT (id) DO UPDATE SET
                 display_name = EXCLUDED.display_name,
                 provider_kind = EXCLUDED.provider_kind,
                 model_name = EXCLUDED.model_name,
                 base_url = EXCLUDED.base_url,
                 credential_ref = EXCLUDED.credential_ref,
                 enabled = EXCLUDED.enabled,
                 config = EXCLUDED.config,
                 updated_at = CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai'",
        )
        .bind(&input.id)
        .bind(&input.display_name)
        .bind(&input.provider_kind)
        .bind(&input.model_name)
        .bind(&input.base_url)
        .bind(&input.credential_ref)
        .bind(input.enabled)
        .bind(&input.config)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn load_model(&self, id: &str) -> Result<Option<ModelRecord>, StorageError> {
        let model = sqlx::query_as::<_, ModelRecord>(
            "SELECT id, display_name, provider_kind, model_name, base_url,
                    credential_ref, enabled, config
             FROM models WHERE id = $1",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(model)
    }
}
