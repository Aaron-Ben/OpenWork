use std::collections::{HashMap, HashSet};

use async_trait::async_trait;
use openwork_models::provider::{
    ApiCredential, ModelTier, ProviderInput, ProviderKind, ProviderModel, ProviderProfile,
    ProviderRepository, ProviderRepositoryError, ProviderRuntimeConfig,
};
use serde_json::{Map, Value, json};
use sqlx::{PgPool, Postgres, Transaction};
use uuid::Uuid;

use super::{ApiKeyCipher, ApiKeyCipherError};

const PROVIDER_COLUMNS: &str =
    "provider_id, display_name, provider_kind, base_url, api_key_encrypted, enabled, config";

#[derive(Debug, sqlx::FromRow)]
struct ProviderCredentialRecord {
    provider_id: String,
    display_name: String,
    provider_kind: String,
    base_url: String,
    api_key_encrypted: String,
    enabled: bool,
    config: Value,
}

#[derive(Debug, sqlx::FromRow)]
struct ProviderModelRecord {
    model_name: String,
    display_name: String,
    enabled: bool,
    config: Value,
}

#[derive(Debug, sqlx::FromRow)]
struct ProviderOwnedModelRecord {
    provider_id: String,
    model_name: String,
    display_name: String,
    enabled: bool,
    config: Value,
}

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

    pub fn from_env(pool: PgPool) -> Result<Self, ApiKeyCipherError> {
        Ok(Self::new(pool, ApiKeyCipher::from_env()?))
    }

    async fn credential_record(
        &self,
        provider_id: &str,
    ) -> Result<Option<ProviderCredentialRecord>, ProviderRepositoryError> {
        let sql =
            format!("SELECT {PROVIDER_COLUMNS} FROM provider_credentials WHERE provider_id = $1");
        sqlx::query_as(&sql)
            .bind(provider_id)
            .fetch_optional(&self.pool)
            .await
            .map_err(persistence_error)
    }

    async fn models_for(
        &self,
        provider_id: &str,
    ) -> Result<Vec<ProviderModel>, ProviderRepositoryError> {
        let credential_ref = credential_ref(provider_id);
        let rows = sqlx::query_as::<_, ProviderModelRecord>(
            "SELECT model_name, display_name, enabled, config
             FROM models
             WHERE credential_ref = $1
             ORDER BY
                 CASE WHEN jsonb_typeof(config->'position') = 'number'
                      THEN (config->>'position')::INTEGER ELSE 0 END,
                 model_name",
        )
        .bind(credential_ref)
        .fetch_all(&self.pool)
        .await
        .map_err(persistence_error)?;
        rows.into_iter().map(record_to_model).collect()
    }

    async fn all_models(
        &self,
    ) -> Result<HashMap<String, Vec<ProviderModel>>, ProviderRepositoryError> {
        let rows = sqlx::query_as::<_, ProviderOwnedModelRecord>(
            "SELECT substring(credential_ref FROM 10) AS provider_id,
                    model_name, display_name, enabled, config
             FROM models
             WHERE credential_ref LIKE 'provider:%'
             ORDER BY credential_ref,
                 CASE WHEN jsonb_typeof(config->'position') = 'number'
                      THEN (config->>'position')::INTEGER ELSE 0 END,
                 model_name",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(persistence_error)?;
        let mut models = HashMap::<String, Vec<ProviderModel>>::new();
        for record in rows {
            let provider_id = record.provider_id;
            models
                .entry(provider_id)
                .or_default()
                .push(record_to_model(ProviderModelRecord {
                    model_name: record.model_name,
                    display_name: record.display_name,
                    enabled: record.enabled,
                    config: record.config,
                })?);
        }
        Ok(models)
    }
}

#[async_trait]
impl ProviderRepository for PostgresProviderRepository {
    async fn list_profiles(&self) -> Result<Vec<ProviderProfile>, ProviderRepositoryError> {
        let sql = format!(
            "SELECT {PROVIDER_COLUMNS} FROM provider_credentials ORDER BY created_at, provider_id"
        );
        let records = sqlx::query_as::<_, ProviderCredentialRecord>(&sql)
            .fetch_all(&self.pool)
            .await
            .map_err(persistence_error)?;
        let mut models = self.all_models().await?;
        records
            .into_iter()
            .map(|record| {
                let provider_models = models.remove(&record.provider_id).unwrap_or_default();
                record_to_profile(record, provider_models)
            })
            .collect()
    }

    async fn get_profile(
        &self,
        id: &str,
    ) -> Result<Option<ProviderProfile>, ProviderRepositoryError> {
        match self.credential_record(id).await? {
            Some(record) => Ok(Some(record_to_profile(record, self.models_for(id).await?)?)),
            None => Ok(None),
        }
    }

    async fn load_runtime(
        &self,
        id: &str,
    ) -> Result<Option<ProviderRuntimeConfig>, ProviderRepositoryError> {
        match self.credential_record(id).await? {
            Some(record) => {
                let credential = ApiCredential::new(
                    self.api_key_cipher
                        .decrypt(id, &record.api_key_encrypted)
                        .map_err(|error| credential_error("decrypt", error))?,
                );
                let adapter_options = extra_body(&record.config);
                let profile = record_to_profile(record, self.models_for(id).await?)?;
                Ok(Some(ProviderRuntimeConfig {
                    profile,
                    credential,
                    adapter_options,
                }))
            }
            None => Ok(None),
        }
    }

    async fn create(
        &self,
        input: ProviderInput,
    ) -> Result<ProviderProfile, ProviderRepositoryError> {
        validate_input(&input, true)?;
        let normalized_models = normalize_models(&input.models);
        let provider_id = format!("prov-{}", Uuid::new_v4().simple());
        let encrypted = self
            .api_key_cipher
            .encrypt(&provider_id, &input.api_key)
            .map_err(|error| credential_error("encrypt", error))?;
        let mut transaction = self.pool.begin().await.map_err(persistence_error)?;
        sqlx::query(
            "INSERT INTO provider_credentials (
                provider_id, display_name, provider_kind, base_url,
                api_key_encrypted, enabled, config
             ) VALUES ($1, $2, $3, $4, $5, $6, $7)",
        )
        .bind(&provider_id)
        .bind(input.name.trim())
        .bind(input.kind.as_str())
        .bind(input.base_url.trim())
        .bind(encrypted)
        .bind(input.enabled)
        .bind(provider_config(input.extra_body.as_ref()))
        .execute(&mut *transaction)
        .await
        .map_err(persistence_error)?;
        replace_models(&mut transaction, &provider_id, &input, &normalized_models).await?;
        transaction.commit().await.map_err(persistence_error)?;
        self.get_profile(&provider_id)
            .await?
            .ok_or(ProviderRepositoryError::NotFound { id: provider_id })
    }

    async fn update(
        &self,
        id: &str,
        input: ProviderInput,
    ) -> Result<ProviderProfile, ProviderRepositoryError> {
        let mut input = input;
        if input.extra_body.is_none() {
            let record = self
                .credential_record(id)
                .await?
                .ok_or_else(|| ProviderRepositoryError::NotFound { id: id.to_string() })?;
            input.extra_body = extra_body(&record.config);
        }
        validate_input(&input, false)?;
        let normalized_models = normalize_models(&input.models);
        let encrypted = if input.api_key.trim().is_empty() {
            None
        } else {
            Some(
                self.api_key_cipher
                    .encrypt(id, &input.api_key)
                    .map_err(|error| credential_error("encrypt", error))?,
            )
        };
        let mut transaction = self.pool.begin().await.map_err(persistence_error)?;
        let result = sqlx::query(
            "UPDATE provider_credentials SET
                display_name = $2,
                provider_kind = $3,
                base_url = $4,
                api_key_encrypted = COALESCE($5, api_key_encrypted),
                enabled = $6,
                config = $7,
                updated_at = CURRENT_TIMESTAMP AT TIME ZONE 'UTC'
             WHERE provider_id = $1",
        )
        .bind(id)
        .bind(input.name.trim())
        .bind(input.kind.as_str())
        .bind(input.base_url.trim())
        .bind(encrypted)
        .bind(input.enabled)
        .bind(provider_config(input.extra_body.as_ref()))
        .execute(&mut *transaction)
        .await
        .map_err(persistence_error)?;
        if result.rows_affected() == 0 {
            return Err(ProviderRepositoryError::NotFound { id: id.to_string() });
        }
        replace_models(&mut transaction, id, &input, &normalized_models).await?;
        transaction.commit().await.map_err(persistence_error)?;
        self.get_profile(id)
            .await?
            .ok_or_else(|| ProviderRepositoryError::NotFound { id: id.to_string() })
    }

    async fn delete(&self, id: &str) -> Result<(), ProviderRepositoryError> {
        let mut transaction = self.pool.begin().await.map_err(persistence_error)?;
        let provider_id: Option<String> = sqlx::query_scalar(
            "SELECT provider_id FROM provider_credentials WHERE provider_id = $1 FOR UPDATE",
        )
        .bind(id)
        .fetch_optional(&mut *transaction)
        .await
        .map_err(persistence_error)?;
        provider_id.ok_or_else(|| ProviderRepositoryError::NotFound { id: id.to_string() })?;
        sqlx::query("DELETE FROM models WHERE credential_ref = $1")
            .bind(credential_ref(id))
            .execute(&mut *transaction)
            .await
            .map_err(persistence_error)?;
        sqlx::query("DELETE FROM provider_credentials WHERE provider_id = $1")
            .bind(id)
            .execute(&mut *transaction)
            .await
            .map_err(persistence_error)?;
        transaction.commit().await.map_err(persistence_error)?;
        Ok(())
    }
}

async fn replace_models(
    transaction: &mut Transaction<'_, Postgres>,
    provider_id: &str,
    provider: &ProviderInput,
    models: &[ProviderModel],
) -> Result<(), ProviderRepositoryError> {
    let reference = credential_ref(provider_id);
    let mut retained_ids = Vec::with_capacity(models.len());
    for (position, model) in models.iter().enumerate() {
        let id = model_record_id(provider_id, &model.model_id);
        retained_ids.push(id.clone());
        let display_name = model
            .display_name
            .as_deref()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or(&model.model_id);
        let config = json!({
            "providerId": provider_id,
            "modelTier": model.model_tier.as_str(),
            "position": position,
            "displayNameProvided": model.display_name.is_some(),
            "modelEnabled": model.enabled,
            "extraBody": provider.extra_body.clone().unwrap_or_default(),
        });
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
                updated_at = CURRENT_TIMESTAMP AT TIME ZONE 'UTC'",
        )
        .bind(id)
        .bind(display_name)
        .bind(provider.kind.as_str())
        .bind(&model.model_id)
        .bind(provider.base_url.trim())
        .bind(&reference)
        .bind(provider.enabled && model.enabled)
        .bind(config)
        .execute(&mut **transaction)
        .await
        .map_err(persistence_error)?;
    }
    sqlx::query(
        "DELETE FROM models
         WHERE credential_ref = $1 AND NOT (id = ANY($2))",
    )
    .bind(&reference)
    .bind(&retained_ids)
    .execute(&mut **transaction)
    .await
    .map_err(persistence_error)?;
    Ok(())
}

fn record_to_profile(
    record: ProviderCredentialRecord,
    models: Vec<ProviderModel>,
) -> Result<ProviderProfile, ProviderRepositoryError> {
    Ok(ProviderProfile {
        id: record.provider_id,
        name: record.display_name,
        base_url: record.base_url,
        kind: parse_provider_kind(&record.provider_kind)?,
        models,
        enabled: record.enabled,
    })
}

fn record_to_model(record: ProviderModelRecord) -> Result<ProviderModel, ProviderRepositoryError> {
    let tier = record
        .config
        .get("modelTier")
        .and_then(Value::as_str)
        .unwrap_or("plus");
    let display_name_provided = record
        .config
        .get("displayNameProvided")
        .and_then(Value::as_bool)
        .unwrap_or(true);
    let model_enabled = record
        .config
        .get("modelEnabled")
        .and_then(Value::as_bool)
        .unwrap_or(record.enabled);
    Ok(ProviderModel {
        model_id: record.model_name,
        display_name: display_name_provided.then_some(record.display_name),
        model_tier: parse_model_tier(tier)?,
        enabled: model_enabled,
    })
}

fn provider_config(extra_body: Option<&Map<String, Value>>) -> Value {
    json!({ "extraBody": extra_body.cloned().unwrap_or_default() })
}

fn extra_body(config: &Value) -> Option<Map<String, Value>> {
    config
        .get("extraBody")
        .and_then(Value::as_object)
        .filter(|value| !value.is_empty())
        .cloned()
}

fn normalize_models(models: &[ProviderModel]) -> Vec<ProviderModel> {
    let mut seen = HashSet::new();
    models
        .iter()
        .filter(|model| seen.insert(model.model_id.trim().to_string()))
        .cloned()
        .collect()
}

fn validate_input(
    input: &ProviderInput,
    require_api_key: bool,
) -> Result<(), ProviderRepositoryError> {
    for (field, value) in [
        ("name", input.name.as_str()),
        ("baseUrl", input.base_url.as_str()),
    ] {
        if value.trim().is_empty() {
            return Err(ProviderRepositoryError::InvalidInput { field });
        }
    }
    if require_api_key && input.api_key.trim().is_empty() {
        return Err(ProviderRepositoryError::InvalidInput { field: "apiKey" });
    }
    if input
        .models
        .iter()
        .any(|model| model.model_id.trim().is_empty())
    {
        return Err(ProviderRepositoryError::InvalidInput {
            field: "models.modelId",
        });
    }
    Ok(())
}

fn credential_ref(provider_id: &str) -> String {
    format!("provider:{provider_id}")
}

fn model_record_id(provider_id: &str, model_id: &str) -> String {
    format!("model:{provider_id}:{model_id}")
}

fn parse_provider_kind(value: &str) -> Result<ProviderKind, ProviderRepositoryError> {
    match value {
        "openai" => Ok(ProviderKind::Openai),
        "anthropic" => Ok(ProviderKind::Anthropic),
        "deepseek" => Ok(ProviderKind::Deepseek),
        "kimi" => Ok(ProviderKind::Kimi),
        "qwen" => Ok(ProviderKind::Qwen),
        "glm" => Ok(ProviderKind::Glm),
        _ => Err(invalid_stored_value("provider_credentials.provider_kind")),
    }
}

fn parse_model_tier(value: &str) -> Result<ModelTier, ProviderRepositoryError> {
    match value {
        "lite" => Ok(ModelTier::Lite),
        "plus" => Ok(ModelTier::Plus),
        "pro" => Ok(ModelTier::Pro),
        _ => Err(invalid_stored_value("models.config.modelTier")),
    }
}

fn invalid_stored_value(field: &'static str) -> ProviderRepositoryError {
    ProviderRepositoryError::Persistence {
        message: format!("invalid stored provider field: {field}"),
    }
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
    use openwork_models::provider::ModelTier;
    use serde_json::json;

    use super::{ProviderModelRecord, record_to_model};

    #[test]
    fn legacy_model_without_tier_does_not_break_the_provider_index() {
        let model = record_to_model(ProviderModelRecord {
            model_name: "legacy-model".to_string(),
            display_name: "Legacy model".to_string(),
            enabled: true,
            config: json!({ "modelEnabled": true }),
        })
        .expect("legacy provider models remain readable");

        assert_eq!(model.model_tier, ModelTier::Plus);
        assert!(model.enabled);
    }
}
