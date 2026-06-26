use openwork_database::{
    Database, DatabaseConfig, DatabaseError, OrderDirection, PgCrud, PgFilterQuery, QueryCriteria,
    now_beijing,
};
use openwork_protocol::ai::{GenerateRequest, Message, Role};
use serde::{Deserialize, Serialize};
use serde_json::Map;
use sqlx::PgPool;
use uuid::Uuid;

use crate::{
    provider_config::{ProviderConfig, ProviderInput, ProviderKind, build_provider},
    records::{PROVIDER_MIGRATIONS, ProviderRecord},
};

/// Provider 索引。前端仍消费这个形状,底层数据来自 PostgreSQL。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderIndex {
    #[serde(default)]
    pub providers: Vec<ProviderConfig>,
    #[serde(default)]
    pub active_id: Option<String>,
}

/// 连通性测试结果。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TestResult {
    pub success: bool,
    pub message: String,
}

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("provider not found: {id}")]
    NotFound { id: String },
    #[error("cannot delete the active provider: {id}")]
    CannotDeleteActive { id: String },
    #[error("provider name is required")]
    EmptyName,
    #[error("provider base url is required")]
    EmptyBaseUrl,
    #[error("provider api key is required")]
    EmptyApiKey,
    #[error("database error: {0}")]
    Database(#[from] DatabaseError),
    #[error("postgres error: {0}")]
    Sqlx(#[from] sqlx::Error),
    #[error("failed to serialize provider store: {0}")]
    Serialize(#[from] serde_json::Error),
}

/// PostgreSQL-backed Provider 配置仓库。
#[derive(Clone)]
pub struct ProviderStore {
    db: Database,
}

impl ProviderStore {
    pub async fn connect_from_env_or_local() -> Result<Self, StoreError> {
        let db = Database::connect(DatabaseConfig::from_env_or_local()).await?;
        let store = Self { db };
        store.db.migrate(PROVIDER_MIGRATIONS).await?;
        Ok(store)
    }

    pub async fn connect(config: DatabaseConfig) -> Result<Self, StoreError> {
        let db = Database::connect(config).await?;
        let store = Self { db };
        store.db.migrate(PROVIDER_MIGRATIONS).await?;
        Ok(store)
    }

    pub fn pool(&self) -> &PgPool {
        self.db.pool()
    }

    pub async fn index(&self) -> Result<ProviderIndex, StoreError> {
        let records = ProviderRecord::find_by_criteria(
            QueryCriteria::new()
                .order_by("created_at", OrderDirection::Asc)
                .order_by("id", OrderDirection::Asc),
            self.pool(),
        )
        .await?;

        let mut providers = Vec::with_capacity(records.len());
        let mut active_id = None;
        for record in records {
            let config = record_to_provider_config(record.clone())?;
            if record.active {
                active_id = Some(config.id.clone());
            }
            providers.push(config);
        }

        Ok(ProviderIndex {
            providers,
            active_id,
        })
    }

    pub async fn list(&self) -> Result<Vec<ProviderConfig>, StoreError> {
        Ok(self.index().await?.providers)
    }

    pub async fn get(&self, id: &str) -> Result<Option<ProviderConfig>, StoreError> {
        ProviderRecord::get_by_id(id.to_string(), self.pool())
            .await?
            .map(record_to_provider_config)
            .transpose()
    }

    pub async fn active(&self) -> Result<Option<ProviderConfig>, StoreError> {
        ProviderRecord::find_one_by_criteria(QueryCriteria::new().eq("active", true), self.pool())
            .await?
            .map(record_to_provider_config)
            .transpose()
    }

    pub async fn add(&self, input: ProviderInput) -> Result<ProviderConfig, StoreError> {
        validate_input(&input)?;
        let now = now_beijing();
        let config = ProviderConfig::new(generate_id(), input);
        let has_active: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM providers WHERE active = true)")
            .fetch_one(self.pool())
            .await?;
        let models_json = serde_json::to_value(&config.data.models)?;
        let extra_body_json = config
            .data
            .extra_body
            .as_ref()
            .map(serde_json::to_value)
            .transpose()?;

        ProviderRecord {
            id: config.id.clone(),
            name: config.data.name.clone(),
            base_url: config.data.base_url.clone(),
            api_key: config.data.api_key.clone(),
            kind: config.data.kind.as_str().to_string(),
            models_json,
            enabled: config.data.enabled,
            extra_body_json,
            active: !has_active,
            created_at: now,
            updated_at: now,
        }
        .create(self.pool())
        .await?;

        Ok(config)
    }

    pub async fn update(
        &self,
        id: &str,
        input: ProviderInput,
    ) -> Result<ProviderConfig, StoreError> {
        validate_input(&input)?;
        let models_json = serde_json::to_value(&input.models)?;
        let extra_body_json = input
            .extra_body
            .as_ref()
            .map(serde_json::to_value)
            .transpose()?;

        let Some(existing) = ProviderRecord::get_by_id(id.to_string(), self.pool()).await? else {
            return Err(StoreError::NotFound { id: id.to_string() });
        };

        ProviderRecord {
            id: id.to_string(),
            name: input.name.clone(),
            base_url: input.base_url.clone(),
            api_key: input.api_key.clone(),
            kind: input.kind.as_str().to_string(),
            models_json,
            enabled: input.enabled,
            extra_body_json,
            active: existing.active,
            created_at: existing.created_at,
            updated_at: now_beijing(),
        }
        .update(self.pool())
        .await?;

        Ok(ProviderConfig::new(id.to_string(), input))
    }

    pub async fn delete(&self, id: &str) -> Result<(), StoreError> {
        let Some(record) = ProviderRecord::get_by_id(id.to_string(), self.pool()).await? else {
            return Err(StoreError::NotFound { id: id.to_string() });
        };
        if record.active {
            return Err(StoreError::CannotDeleteActive { id: id.to_string() });
        }
        record.delete(self.pool()).await?;
        Ok(())
    }

    pub async fn activate(&self, id: &str) -> Result<(), StoreError> {
        let mut tx = self.pool().begin().await?;
        let exists: Option<i64> = sqlx::query_scalar("SELECT 1::BIGINT FROM providers WHERE id = $1")
            .bind(id)
            .fetch_optional(&mut *tx)
            .await?;
        if exists.is_none() {
            return Err(StoreError::NotFound { id: id.to_string() });
        }

        sqlx::query("UPDATE providers SET active = false WHERE active = true")
            .execute(&mut *tx)
            .await?;
        sqlx::query("UPDATE providers SET active = true, updated_at = $1 WHERE id = $2")
            .bind(now_beijing())
            .bind(id)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(())
    }

    pub async fn clear_active(&self) -> Result<(), StoreError> {
        sqlx::query("UPDATE providers SET active = false WHERE active = true")
            .execute(self.pool())
            .await?;
        Ok(())
    }
}

/// 发一个最小请求验证 provider 配置可用。不依赖 store 内部状态,可用于测试未保存的配置。
pub async fn test_provider(config: &ProviderConfig, model: &str) -> TestResult {
    let provider = build_provider(config);
    let request = GenerateRequest {
        model: model.to_string(),
        messages: vec![Message::text(Role::User, "ping")],
        temperature: None,
        max_tokens: Some(16),
        stream: false,
        thinking: None,
        tools: Vec::new(),
    };
    match provider.generate(request).await {
        Ok(_) => TestResult {
            success: true,
            message: "Connectivity OK".to_string(),
        },
        Err(error) => TestResult {
            success: false,
            message: error.to_string(),
        },
    }
}

fn validate_input(input: &ProviderInput) -> Result<(), StoreError> {
    if input.name.trim().is_empty() {
        return Err(StoreError::EmptyName);
    }
    if input.base_url.trim().is_empty() {
        return Err(StoreError::EmptyBaseUrl);
    }
    if input.api_key.trim().is_empty() {
        return Err(StoreError::EmptyApiKey);
    }
    Ok(())
}

fn record_to_provider_config(record: ProviderRecord) -> Result<ProviderConfig, StoreError> {
    let extra_body: Option<Map<String, serde_json::Value>> = record
        .extra_body_json
        .map(serde_json::from_value)
        .transpose()?;

    Ok(ProviderConfig::new(
        record.id,
        ProviderInput {
            name: record.name,
            base_url: record.base_url,
            api_key: record.api_key,
            kind: kind_from_str(&record.kind)?,
            models: serde_json::from_value(record.models_json).unwrap_or_default(),
            enabled: record.enabled,
            extra_body,
        },
    ))
}

fn kind_from_str(input: &str) -> Result<ProviderKind, serde_json::Error> {
    serde_json::from_value(serde_json::Value::String(input.to_string()))
}

fn generate_id() -> String {
    format!("prov-{}", Uuid::new_v4().simple())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider_config::ProviderKind;

    async fn test_store() -> Option<ProviderStore> {
        let url = std::env::var("TEST_DATABASE_URL").ok()?;
        let store = ProviderStore::connect(DatabaseConfig {
            url,
            max_connections: 2,
            acquire_timeout: std::time::Duration::from_secs(5),
        })
        .await
        .ok()?;
        let _ = sqlx::query("TRUNCATE TABLE providers CASCADE")
            .execute(store.pool())
            .await;
        Some(store)
    }

    fn sample_input() -> ProviderInput {
        ProviderInput {
            name: "DeepSeek".to_string(),
            base_url: "https://api.deepseek.com".to_string(),
            api_key: "sk-test".to_string(),
            kind: ProviderKind::Deepseek,
            models: vec!["deepseek-chat".to_string()],
            enabled: true,
            extra_body: None,
        }
    }

    #[tokio::test]
    async fn add_persists_and_assigns_id() {
        let Some(store) = test_store().await else {
            return;
        };
        let config = store.add(sample_input()).await.unwrap();

        assert!(config.id.starts_with("prov-"));
        assert_eq!(store.list().await.unwrap().len(), 1);
        assert_eq!(store.get(&config.id).await.unwrap().unwrap().id, config.id);
    }

    #[tokio::test]
    async fn update_replaces_fields() {
        let Some(store) = test_store().await else {
            return;
        };
        let config = store.add(sample_input()).await.unwrap();

        let mut input = sample_input();
        input.name = "Renamed".to_string();
        let updated = store.update(&config.id, input).await.unwrap();

        assert_eq!(updated.data.name, "Renamed");
        assert_eq!(
            store.get(&config.id).await.unwrap().unwrap().data.name,
            "Renamed"
        );
    }

    #[tokio::test]
    async fn activate_sets_active_id() {
        let Some(store) = test_store().await else {
            return;
        };
        let config = store.add(sample_input()).await.unwrap();

        store.activate(&config.id).await.unwrap();
        assert_eq!(
            store.index().await.unwrap().active_id,
            Some(config.id.clone())
        );
        assert_eq!(store.active().await.unwrap().map(|c| c.id), Some(config.id));
    }

    #[tokio::test]
    async fn delete_refuses_active_provider() {
        let Some(store) = test_store().await else {
            return;
        };
        let config = store.add(sample_input()).await.unwrap();
        store.activate(&config.id).await.unwrap();

        let error = store.delete(&config.id).await.unwrap_err();
        assert!(matches!(error, StoreError::CannotDeleteActive { .. }));
    }

    #[tokio::test]
    async fn add_rejects_empty_fields() {
        let Some(store) = test_store().await else {
            return;
        };

        let mut empty_name = sample_input();
        empty_name.name = "  ".to_string();
        assert!(matches!(
            store.add(empty_name).await.unwrap_err(),
            StoreError::EmptyName
        ));

        let mut empty_url = sample_input();
        empty_url.base_url = String::new();
        assert!(matches!(
            store.add(empty_url).await.unwrap_err(),
            StoreError::EmptyBaseUrl
        ));

        assert!(store.list().await.unwrap().is_empty());
    }
}
