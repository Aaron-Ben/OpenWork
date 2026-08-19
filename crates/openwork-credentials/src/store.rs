use serde_json::Value;
use sqlx::{PgConnection, PgPool};
use thiserror::Error;
use zeroize::Zeroizing;

use crate::{ApiKeyCipher, ApiKeyCipherError};

const COLUMNS: &str =
    "provider_id, display_name, provider_kind, base_url, api_key_encrypted, enabled, config";

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct ProviderCredentialRecord {
    pub provider_id: String,
    pub display_name: String,
    pub provider_kind: String,
    pub base_url: String,
    pub api_key_encrypted: String,
    pub enabled: bool,
    pub config: Value,
}

#[derive(Clone)]
pub struct DecryptedProviderCredential {
    pub provider_id: String,
    pub display_name: String,
    pub provider_kind: String,
    pub base_url: String,
    api_key: Zeroizing<String>,
    pub enabled: bool,
    pub config: Value,
}

impl DecryptedProviderCredential {
    pub fn api_key(&self) -> &str {
        self.api_key.as_str()
    }
}

impl std::fmt::Debug for DecryptedProviderCredential {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("DecryptedProviderCredential")
            .field("provider_id", &self.provider_id)
            .field("display_name", &self.display_name)
            .field("provider_kind", &self.provider_kind)
            .field("base_url", &self.base_url)
            .field("api_key", &"[REDACTED]")
            .field("enabled", &self.enabled)
            .field("config", &self.config)
            .finish()
    }
}

#[derive(Clone, Copy)]
pub struct ProviderCredentialInput<'a> {
    pub provider_id: &'a str,
    pub display_name: &'a str,
    pub provider_kind: &'a str,
    pub base_url: &'a str,
    pub api_key: Option<&'a str>,
    pub enabled: bool,
    pub config: &'a Value,
}

impl std::fmt::Debug for ProviderCredentialInput<'_> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ProviderCredentialInput")
            .field("provider_id", &self.provider_id)
            .field("display_name", &self.display_name)
            .field("provider_kind", &self.provider_kind)
            .field("base_url", &self.base_url)
            .field("api_key", &self.api_key.map(|_| "[REDACTED]"))
            .field("enabled", &self.enabled)
            .field("config", &self.config)
            .finish()
    }
}

#[derive(Clone)]
pub struct PostgresCredentialStore {
    pool: PgPool,
    cipher: ApiKeyCipher,
}

impl PostgresCredentialStore {
    pub fn new(pool: PgPool, cipher: ApiKeyCipher) -> Self {
        Self { pool, cipher }
    }

    pub fn from_env(pool: PgPool) -> Result<Self, ProviderCredentialStoreError> {
        Ok(Self::new(pool, ApiKeyCipher::from_env()?))
    }

    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    pub async fn list(
        &self,
    ) -> Result<Vec<ProviderCredentialRecord>, ProviderCredentialStoreError> {
        let sql =
            format!("SELECT {COLUMNS} FROM provider_credentials ORDER BY created_at, provider_id");
        Ok(sqlx::query_as(&sql).fetch_all(&self.pool).await?)
    }

    pub async fn get(
        &self,
        provider_id: &str,
    ) -> Result<Option<ProviderCredentialRecord>, ProviderCredentialStoreError> {
        let sql = format!("SELECT {COLUMNS} FROM provider_credentials WHERE provider_id = $1");
        Ok(sqlx::query_as(&sql)
            .bind(provider_id)
            .fetch_optional(&self.pool)
            .await?)
    }

    pub async fn load(
        &self,
        provider_id: &str,
    ) -> Result<Option<DecryptedProviderCredential>, ProviderCredentialStoreError> {
        self.get(provider_id)
            .await?
            .map(|record| self.decrypt_record(record))
            .transpose()
    }

    pub fn decrypt_record(
        &self,
        record: ProviderCredentialRecord,
    ) -> Result<DecryptedProviderCredential, ProviderCredentialStoreError> {
        let api_key = self
            .cipher
            .decrypt(&record.provider_id, &record.api_key_encrypted)?;
        Ok(DecryptedProviderCredential {
            provider_id: record.provider_id,
            display_name: record.display_name,
            provider_kind: record.provider_kind,
            base_url: record.base_url,
            api_key: Zeroizing::new(api_key),
            enabled: record.enabled,
            config: record.config,
        })
    }

    pub async fn insert(
        &self,
        connection: &mut PgConnection,
        input: ProviderCredentialInput<'_>,
    ) -> Result<(), ProviderCredentialStoreError> {
        let api_key = input
            .api_key
            .ok_or(ApiKeyCipherError::BlankField { field: "api_key" })?;
        let encrypted = self.cipher.encrypt(input.provider_id, api_key)?;
        sqlx::query(
            "INSERT INTO provider_credentials (
                provider_id, display_name, provider_kind, base_url,
                api_key_encrypted, enabled, config
             ) VALUES ($1, $2, $3, $4, $5, $6, $7)",
        )
        .bind(input.provider_id)
        .bind(input.display_name)
        .bind(input.provider_kind)
        .bind(input.base_url)
        .bind(encrypted)
        .bind(input.enabled)
        .bind(input.config)
        .execute(connection)
        .await?;
        Ok(())
    }

    pub async fn update(
        &self,
        connection: &mut PgConnection,
        input: ProviderCredentialInput<'_>,
    ) -> Result<u64, ProviderCredentialStoreError> {
        let encrypted = input
            .api_key
            .map(|api_key| self.cipher.encrypt(input.provider_id, api_key))
            .transpose()?;
        let result = sqlx::query(
            "UPDATE provider_credentials SET
                display_name = $2,
                provider_kind = $3,
                base_url = $4,
                api_key_encrypted = COALESCE($5, api_key_encrypted),
                enabled = $6,
                config = $7,
                updated_at = CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai'
             WHERE provider_id = $1",
        )
        .bind(input.provider_id)
        .bind(input.display_name)
        .bind(input.provider_kind)
        .bind(input.base_url)
        .bind(encrypted)
        .bind(input.enabled)
        .bind(input.config)
        .execute(connection)
        .await?;
        Ok(result.rows_affected())
    }

    pub async fn lock_exists(
        &self,
        connection: &mut PgConnection,
        provider_id: &str,
    ) -> Result<bool, ProviderCredentialStoreError> {
        let found: Option<String> = sqlx::query_scalar(
            "SELECT provider_id FROM provider_credentials WHERE provider_id = $1 FOR UPDATE",
        )
        .bind(provider_id)
        .fetch_optional(connection)
        .await?;
        Ok(found.is_some())
    }

    pub async fn delete(
        &self,
        connection: &mut PgConnection,
        provider_id: &str,
    ) -> Result<u64, ProviderCredentialStoreError> {
        let result = sqlx::query("DELETE FROM provider_credentials WHERE provider_id = $1")
            .bind(provider_id)
            .execute(connection)
            .await?;
        Ok(result.rows_affected())
    }
}

#[derive(Debug, Error)]
pub enum ProviderCredentialStoreError {
    #[error(transparent)]
    Cipher(#[from] ApiKeyCipherError),
    #[error("provider credential persistence failed: {0}")]
    Persistence(#[from] sqlx::Error),
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use zeroize::Zeroizing;

    use super::DecryptedProviderCredential;

    #[test]
    fn decrypted_record_debug_is_redacted() {
        let record = DecryptedProviderCredential {
            provider_id: "provider-a".to_string(),
            display_name: "Provider A".to_string(),
            provider_kind: "openai".to_string(),
            base_url: "https://example.invalid".to_string(),
            api_key: Zeroizing::new("never-print-me".to_string()),
            enabled: true,
            config: json!({}),
        };
        let debug = format!("{record:?}");
        assert!(debug.contains("[REDACTED]"));
        assert!(!debug.contains("never-print-me"));
    }
}
