use std::collections::{HashMap, HashSet};

use async_trait::async_trait;
use openwork_protocol::provider::{
    ApiCredential, ModelTier, ProviderInput, ProviderKind, ProviderModel, ProviderProfile,
    ProviderRepository, ProviderRepositoryError, ProviderRuntimeConfig,
};
use serde_json::Map;
use sqlx::{PgPool, Postgres, Transaction};
use uuid::Uuid;

use crate::{ApiKeyCipher, ApiKeyCipherError};

use super::record::{ProviderModelRecord, ProviderProfileRecord, ProviderRuntimeRecord};

const PROFILE_COLUMNS: &str = "id, name, base_url, driver_code, enabled";
const RUNTIME_COLUMNS: &str =
    "id, name, base_url, api_key_encrypted, driver_code, enabled, adapter_options_json";

#[derive(Clone)]
pub struct PostgresProviderRepository {
    pool: PgPool,
    api_key_cipher: ApiKeyCipher,
}

impl PostgresProviderRepository {
    pub fn new(pool: PgPool, api_key_cipher: ApiKeyCipher) -> Self {
        Self {
            pool,
            api_key_cipher,
        }
    }

    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    async fn models_for(
        &self,
        provider_id: &str,
    ) -> Result<Vec<ProviderModel>, ProviderRepositoryError> {
        let rows = sqlx::query_as::<_, ProviderModelRecord>(
            "SELECT provider_id, model_id, display_name, model_tier, enabled
             FROM provider_models
             WHERE provider_id = $1 AND NOT is_deleted
             ORDER BY position ASC",
        )
        .bind(provider_id)
        .fetch_all(self.pool())
        .await
        .map_err(persistence_error)?;
        rows.into_iter().map(record_to_model).collect()
    }

    async fn all_models(
        &self,
    ) -> Result<HashMap<String, Vec<ProviderModel>>, ProviderRepositoryError> {
        let rows = sqlx::query_as::<_, ProviderModelRecord>(
            "SELECT provider_id, model_id, display_name, model_tier, enabled
             FROM provider_models
             WHERE NOT is_deleted
             ORDER BY provider_id ASC, position ASC",
        )
        .fetch_all(self.pool())
        .await
        .map_err(persistence_error)?;
        let mut models = HashMap::<String, Vec<ProviderModel>>::new();
        for row in rows {
            let provider_id = row.provider_id.clone();
            models
                .entry(provider_id)
                .or_default()
                .push(record_to_model(row)?);
        }
        Ok(models)
    }
}

#[async_trait]
impl ProviderRepository for PostgresProviderRepository {
    async fn list_profiles(&self) -> Result<Vec<ProviderProfile>, ProviderRepositoryError> {
        let sql = format!(
            "SELECT {PROFILE_COLUMNS} FROM providers
             WHERE NOT is_deleted
             ORDER BY created_at ASC, id ASC"
        );
        let records = sqlx::query_as::<_, ProviderProfileRecord>(&sql)
            .fetch_all(self.pool())
            .await
            .map_err(persistence_error)?;
        let mut models = self.all_models().await?;
        let mut providers = Vec::with_capacity(records.len());
        for record in records {
            let provider_models = models.remove(&record.id).unwrap_or_default();
            providers.push(record_to_profile(record, provider_models)?);
        }
        Ok(providers)
    }

    async fn get_profile(
        &self,
        id: &str,
    ) -> Result<Option<ProviderProfile>, ProviderRepositoryError> {
        let sql =
            format!("SELECT {PROFILE_COLUMNS} FROM providers WHERE id = $1 AND NOT is_deleted");
        let record = sqlx::query_as::<_, ProviderProfileRecord>(&sql)
            .bind(id)
            .fetch_optional(self.pool())
            .await
            .map_err(persistence_error)?;
        match record {
            Some(record) => Ok(Some(record_to_profile(record, self.models_for(id).await?)?)),
            None => Ok(None),
        }
    }

    async fn load_runtime(
        &self,
        id: &str,
    ) -> Result<Option<ProviderRuntimeConfig>, ProviderRepositoryError> {
        let sql =
            format!("SELECT {RUNTIME_COLUMNS} FROM providers WHERE id = $1 AND NOT is_deleted");
        let record = sqlx::query_as::<_, ProviderRuntimeRecord>(&sql)
            .bind(id)
            .fetch_optional(self.pool())
            .await
            .map_err(persistence_error)?;
        match record {
            Some(record) => {
                let credential = ApiCredential::new(
                    self.api_key_cipher
                        .decrypt(id, &record.api_key_encrypted)
                        .map_err(|error| credential_error("decrypt", error))?,
                );
                Ok(Some(record_to_runtime(
                    record,
                    self.models_for(id).await?,
                    credential,
                )?))
            }
            None => Ok(None),
        }
    }

    async fn active_id(&self) -> Result<Option<String>, ProviderRepositoryError> {
        sqlx::query_scalar("SELECT id FROM providers WHERE active = true AND NOT is_deleted")
            .fetch_optional(self.pool())
            .await
            .map_err(persistence_error)
    }

    async fn create(
        &self,
        input: ProviderInput,
    ) -> Result<ProviderProfile, ProviderRepositoryError> {
        validate_input(&input)?;
        let mut normalized = input;
        normalized.models = normalize_models(&normalized.models);
        let id = generate_id();
        let adapter_options_json = normalized
            .extra_body
            .as_ref()
            .map(serde_json::to_value)
            .transpose()
            .map_err(persistence_error)?;
        let api_key_encrypted = self
            .api_key_cipher
            .encrypt(&id, &normalized.api_key)
            .map_err(|error| credential_error("encrypt", error))?;
        let mut tx = self.pool().begin().await.map_err(persistence_error)?;
        sqlx::query("LOCK TABLE providers IN SHARE ROW EXCLUSIVE MODE")
            .execute(&mut *tx)
            .await
            .map_err(persistence_error)?;
        let has_active: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM providers WHERE active = true AND NOT is_deleted)",
        )
        .fetch_one(&mut *tx)
        .await
        .map_err(persistence_error)?;
        sqlx::query(
            "INSERT INTO providers
             (id, name, base_url, api_key_encrypted, driver_code, enabled, adapter_options_json, active)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
        )
        .bind(&id)
        .bind(&normalized.name)
        .bind(&normalized.base_url)
        .bind(api_key_encrypted)
        .bind(normalized.kind.driver_code())
        .bind(normalized.enabled)
        .bind(adapter_options_json)
        .bind(!has_active)
        .execute(&mut *tx)
        .await
        .map_err(persistence_error)?;
        replace_models(&mut tx, &id, &normalized.models).await?;
        tx.commit().await.map_err(persistence_error)?;
        Ok(input_to_profile(id, &normalized))
    }

    async fn update(
        &self,
        id: &str,
        input: ProviderInput,
    ) -> Result<ProviderProfile, ProviderRepositoryError> {
        validate_input(&input)?;
        let mut normalized = input;
        normalized.models = normalize_models(&normalized.models);
        let adapter_options_json = normalized
            .extra_body
            .as_ref()
            .map(serde_json::to_value)
            .transpose()
            .map_err(persistence_error)?;
        let mut tx = self.pool().begin().await.map_err(persistence_error)?;
        let exists = sqlx::query_scalar::<_, i32>(
            "SELECT 1 FROM providers WHERE id = $1 AND NOT is_deleted FOR UPDATE",
        )
        .bind(id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(persistence_error)?
        .is_some();
        if !exists {
            return Err(ProviderRepositoryError::NotFound { id: id.to_string() });
        }
        let api_key_encrypted = self
            .api_key_cipher
            .encrypt(id, &normalized.api_key)
            .map_err(|error| credential_error("encrypt", error))?;
        let result = sqlx::query(
            "UPDATE providers
             SET name = $1, base_url = $2, api_key_encrypted = $3, driver_code = $4, enabled = $5,
                 adapter_options_json = $6,
                 updated_at = CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai'
             WHERE id = $7 AND NOT is_deleted",
        )
        .bind(&normalized.name)
        .bind(&normalized.base_url)
        .bind(api_key_encrypted)
        .bind(normalized.kind.driver_code())
        .bind(normalized.enabled)
        .bind(adapter_options_json)
        .bind(id)
        .execute(&mut *tx)
        .await
        .map_err(persistence_error)?;
        if result.rows_affected() == 0 {
            return Err(ProviderRepositoryError::NotFound { id: id.to_string() });
        }
        replace_models(&mut tx, id, &normalized.models).await?;
        tx.commit().await.map_err(persistence_error)?;
        Ok(input_to_profile(id.to_string(), &normalized))
    }

    async fn delete(&self, id: &str) -> Result<(), ProviderRepositoryError> {
        let mut tx = self.pool().begin().await.map_err(persistence_error)?;
        let active = sqlx::query_scalar::<_, bool>(
            "SELECT active FROM providers WHERE id = $1 AND NOT is_deleted FOR UPDATE",
        )
        .bind(id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(persistence_error)?
        .ok_or_else(|| ProviderRepositoryError::NotFound { id: id.to_string() })?;
        if active {
            return Err(ProviderRepositoryError::CannotDeleteActive { id: id.to_string() });
        }
        sqlx::query(
            "UPDATE provider_models
             SET is_deleted = true,
                 deleted_at = CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai',
                 updated_at = CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai'
             WHERE provider_id = $1 AND NOT is_deleted",
        )
        .bind(id)
        .execute(&mut *tx)
        .await
        .map_err(persistence_error)?;
        sqlx::query(
            "UPDATE providers
             SET is_deleted = true,
                 deleted_at = CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai',
                 updated_at = CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai'
             WHERE id = $1 AND NOT is_deleted",
        )
        .bind(id)
        .execute(&mut *tx)
        .await
        .map_err(persistence_error)?;
        tx.commit().await.map_err(persistence_error)?;
        Ok(())
    }

    async fn activate(&self, id: &str) -> Result<(), ProviderRepositoryError> {
        let mut tx = self.pool().begin().await.map_err(persistence_error)?;
        sqlx::query("LOCK TABLE providers IN SHARE ROW EXCLUSIVE MODE")
            .execute(&mut *tx)
            .await
            .map_err(persistence_error)?;
        let exists: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM providers WHERE id = $1 AND NOT is_deleted)",
        )
        .bind(id)
        .fetch_one(&mut *tx)
        .await
        .map_err(persistence_error)?;
        if !exists {
            return Err(ProviderRepositoryError::NotFound { id: id.to_string() });
        }
        sqlx::query(
            "UPDATE providers
             SET active = false,
                 updated_at = CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai'
             WHERE active = true AND NOT is_deleted",
        )
        .execute(&mut *tx)
        .await
        .map_err(persistence_error)?;
        sqlx::query(
            "UPDATE providers
             SET active = true,
                 updated_at = CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai'
             WHERE id = $1",
        )
        .bind(id)
        .execute(&mut *tx)
        .await
        .map_err(persistence_error)?;
        tx.commit().await.map_err(persistence_error)?;
        Ok(())
    }

    async fn clear_active(&self) -> Result<(), ProviderRepositoryError> {
        sqlx::query(
            "UPDATE providers
             SET active = false,
                 updated_at = CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai'
             WHERE active = true AND NOT is_deleted",
        )
        .execute(self.pool())
        .await
        .map_err(persistence_error)?;
        Ok(())
    }
}

async fn replace_models(
    tx: &mut Transaction<'_, Postgres>,
    provider_id: &str,
    models: &[ProviderModel],
) -> Result<(), ProviderRepositoryError> {
    sqlx::query(
        "UPDATE provider_models
         SET is_deleted = true,
             deleted_at = CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai',
             updated_at = CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai'
         WHERE provider_id = $1 AND NOT is_deleted",
    )
    .bind(provider_id)
    .execute(&mut **tx)
    .await
    .map_err(persistence_error)?;
    for (position, model) in models.iter().enumerate() {
        sqlx::query(
            "INSERT INTO provider_models
             (provider_id, model_id, display_name, model_tier, position, enabled)
             VALUES ($1, $2, $3, $4, $5, $6)
             ON CONFLICT (provider_id, model_id) DO UPDATE SET
               display_name = EXCLUDED.display_name,
               model_tier = EXCLUDED.model_tier,
               position = EXCLUDED.position,
               enabled = EXCLUDED.enabled,
               updated_at = CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai',
               is_deleted = false,
               deleted_at = NULL",
        )
        .bind(provider_id)
        .bind(&model.model_id)
        .bind(&model.display_name)
        .bind(model.model_tier.as_str())
        .bind(position as i32)
        .bind(model.enabled)
        .execute(&mut **tx)
        .await
        .map_err(persistence_error)?;
    }
    Ok(())
}

fn record_to_model(record: ProviderModelRecord) -> Result<ProviderModel, ProviderRepositoryError> {
    Ok(ProviderModel {
        model_id: record.model_id,
        display_name: record.display_name,
        model_tier: parse_model_tier(&record.model_tier)?,
        enabled: record.enabled,
    })
}

fn record_to_profile(
    record: ProviderProfileRecord,
    models: Vec<ProviderModel>,
) -> Result<ProviderProfile, ProviderRepositoryError> {
    let kind = parse_provider_kind(&record.driver_code)?;
    Ok(ProviderProfile {
        id: record.id,
        name: record.name,
        base_url: record.base_url,
        kind,
        models,
        enabled: record.enabled,
    })
}

fn record_to_runtime(
    record: ProviderRuntimeRecord,
    models: Vec<ProviderModel>,
    credential: ApiCredential,
) -> Result<ProviderRuntimeConfig, ProviderRepositoryError> {
    let extra_body: Option<Map<String, serde_json::Value>> = record
        .adapter_options_json
        .map(serde_json::from_value)
        .transpose()
        .map_err(persistence_error)?;
    let kind = parse_provider_kind(&record.driver_code)?;
    Ok(ProviderRuntimeConfig {
        profile: ProviderProfile {
            id: record.id,
            name: record.name,
            base_url: record.base_url,
            kind,
            models,
            enabled: record.enabled,
        },
        credential,
        adapter_options: extra_body,
    })
}

fn parse_provider_kind(driver_code: &str) -> Result<ProviderKind, ProviderRepositoryError> {
    ProviderKind::from_driver_code(driver_code).ok_or_else(|| {
        ProviderRepositoryError::Persistence {
            message: format!("unknown provider driver code: {driver_code}"),
        }
    })
}

fn input_to_profile(id: String, input: &ProviderInput) -> ProviderProfile {
    ProviderProfile {
        id,
        name: input.name.clone(),
        base_url: input.base_url.clone(),
        kind: input.kind,
        models: input.models.clone(),
        enabled: input.enabled,
    }
}

fn validate_input(input: &ProviderInput) -> Result<(), ProviderRepositoryError> {
    for (field, value) in [
        ("name", input.name.as_str()),
        ("base_url", input.base_url.as_str()),
        ("api_key", input.api_key.as_str()),
    ] {
        if value.trim().is_empty() {
            return Err(ProviderRepositoryError::InvalidInput { field });
        }
    }
    Ok(())
}

fn normalize_models(models: &[ProviderModel]) -> Vec<ProviderModel> {
    let mut seen = HashSet::new();
    models
        .iter()
        .filter_map(|model| {
            let model_id = model.model_id.trim();
            if model_id.is_empty() || !seen.insert(model_id.to_string()) {
                return None;
            }
            Some(ProviderModel {
                model_id: model_id.to_string(),
                display_name: model
                    .display_name
                    .as_deref()
                    .map(str::trim)
                    .filter(|name| !name.is_empty())
                    .map(ToOwned::to_owned),
                model_tier: model.model_tier,
                enabled: model.enabled,
            })
        })
        .collect()
}

fn parse_model_tier(value: &str) -> Result<ModelTier, ProviderRepositoryError> {
    match value {
        "lite" => Ok(ModelTier::Lite),
        "plus" => Ok(ModelTier::Plus),
        "pro" => Ok(ModelTier::Pro),
        _ => Err(ProviderRepositoryError::Persistence {
            message: format!("unknown model tier: {value}"),
        }),
    }
}

fn generate_id() -> String {
    format!("prov-{}", Uuid::new_v4().simple())
}

fn persistence_error(error: impl std::fmt::Display) -> ProviderRepositoryError {
    ProviderRepositoryError::Persistence {
        message: error.to_string(),
    }
}

fn credential_error(operation: &'static str, error: ApiKeyCipherError) -> ProviderRepositoryError {
    ProviderRepositoryError::CredentialEncryption {
        operation,
        message: error.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profile_queries_cannot_select_credentials() {
        assert!(
            !PROFILE_COLUMNS
                .split(',')
                .any(|column| column.trim() == "api_key")
        );
        assert!(
            RUNTIME_COLUMNS
                .split(',')
                .any(|column| column.trim() == "api_key_encrypted")
        );
        assert!(
            !RUNTIME_COLUMNS
                .split(',')
                .any(|column| column.trim() == "api_key")
        );
    }

    #[test]
    fn model_normalization_preserves_order_and_removes_duplicates() {
        let models = normalize_models(&[
            ProviderModel {
                model_id: "model-b".to_string(),
                display_name: None,
                model_tier: ModelTier::Lite,
                enabled: true,
            },
            ProviderModel {
                model_id: " model-a ".to_string(),
                display_name: Some(" Model A ".to_string()),
                model_tier: ModelTier::Pro,
                enabled: true,
            },
            ProviderModel {
                model_id: "model-b".to_string(),
                display_name: None,
                model_tier: ModelTier::Plus,
                enabled: true,
            },
            ProviderModel {
                model_id: "".to_string(),
                display_name: None,
                model_tier: ModelTier::Plus,
                enabled: true,
            },
        ]);
        assert_eq!(models.len(), 2);
        assert_eq!(models[0].model_id, "model-b");
        assert_eq!(models[0].model_tier, ModelTier::Lite);
        assert_eq!(models[1].model_id, "model-a");
        assert_eq!(models[1].display_name.as_deref(), Some("Model A"));
    }
}
