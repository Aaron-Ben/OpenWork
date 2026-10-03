//! Provider 配置的文件存储：`~/.openwork/config.json` 的 `providers` 键。
//!
//! 明文保存 API key（用户决定，2026-10-03），只靠文件权限 `0600` 保护。写入先写临时文件，
//! 再改名替换，进程内用互斥锁串行化。`providers` 以外的键原样保留。

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use async_trait::async_trait;
use openwork_models::provider::{
    ApiCredential, ProviderInput, ProviderProfile, ProviderRepository, ProviderRepositoryError,
    ProviderRuntimeConfig, ProviderSettings,
};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use tokio::sync::Mutex;

/// 模型引用 `<providerId>/<modelId>` 的分隔符。Provider id 中不能出现它，模型 id 中可以。
pub const MODEL_REF_SEPARATOR: char = '/';

/// 一个模型引用：哪个 Provider 配置下的哪个模型。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelRef {
    pub provider_id: String,
    pub model_id: String,
}

impl ModelRef {
    /// 按第一个 `/` 切开。两侧都不能为空。
    pub fn parse(value: &str) -> Option<Self> {
        let (provider_id, model_id) = value.split_once(MODEL_REF_SEPARATOR)?;
        if provider_id.trim().is_empty() || model_id.trim().is_empty() {
            return None;
        }
        Some(Self {
            provider_id: provider_id.to_string(),
            model_id: model_id.to_string(),
        })
    }

    pub fn to_ref_string(&self) -> String {
        format!("{}{MODEL_REF_SEPARATOR}{}", self.provider_id, self.model_id)
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct ConfigFile {
    #[serde(default)]
    providers: BTreeMap<String, StoredProvider>,
    #[serde(flatten)]
    other: Map<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StoredProvider {
    #[serde(flatten)]
    settings: ProviderSettings,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    api_key: Option<String>,
}

impl StoredProvider {
    fn profile(&self, id: &str) -> ProviderProfile {
        ProviderProfile {
            id: id.to_string(),
            settings: self.settings.clone(),
            has_api_key: self.api_key.as_deref().is_some_and(|key| !key.is_empty()),
        }
    }

    /// 直接写入的 key 优先，否则读 `envKey` 指向的环境变量。
    fn credential(&self) -> Option<ApiCredential> {
        self.api_key
            .clone()
            .filter(|key| !key.is_empty())
            .or_else(|| {
                self.settings
                    .env_key
                    .as_deref()
                    .and_then(|name| std::env::var(name).ok())
                    .filter(|key| !key.is_empty())
            })
            .map(ApiCredential::new)
    }
}

pub struct FileProviderRepository {
    path: PathBuf,
    write: Mutex<()>,
}

impl FileProviderRepository {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            write: Mutex::new(()),
        }
    }

    /// 默认路径 `~/.openwork/config.json`。
    pub fn default_path() -> Option<PathBuf> {
        std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".openwork/config.json"))
    }

    async fn read(&self) -> Result<ConfigFile, ProviderRepositoryError> {
        match tokio::fs::read(&self.path).await {
            Ok(bytes) => {
                serde_json::from_slice(&bytes).map_err(|error| persistence(&self.path, error))
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(ConfigFile::default()),
            Err(error) => Err(persistence(&self.path, error)),
        }
    }

    async fn write(&self, config: &ConfigFile) -> Result<(), ProviderRepositoryError> {
        let bytes =
            serde_json::to_vec_pretty(config).map_err(|error| persistence(&self.path, error))?;
        if let Some(parent) = self.path.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|error| persistence(parent, error))?;
        }
        let temporary = self.path.with_extension("json.tmp");
        tokio::fs::write(&temporary, &bytes)
            .await
            .map_err(|error| persistence(&temporary, error))?;
        restrict_permissions(&temporary).await?;
        tokio::fs::rename(&temporary, &self.path)
            .await
            .map_err(|error| persistence(&self.path, error))
    }
}

#[cfg(unix)]
async fn restrict_permissions(path: &Path) -> Result<(), ProviderRepositoryError> {
    use std::os::unix::fs::PermissionsExt;
    tokio::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
        .await
        .map_err(|error| persistence(path, error))
}

#[cfg(not(unix))]
async fn restrict_permissions(_path: &Path) -> Result<(), ProviderRepositoryError> {
    Ok(())
}

fn persistence(path: &Path, error: impl std::fmt::Display) -> ProviderRepositoryError {
    ProviderRepositoryError::Persistence {
        message: format!("{}: {error}", path.display()),
    }
}

fn validate(id: &str, settings: &ProviderSettings) -> Result<(), ProviderRepositoryError> {
    if id.trim().is_empty() || id.contains(MODEL_REF_SEPARATOR) {
        return Err(ProviderRepositoryError::InvalidInput { field: "id" });
    }
    if settings.name.trim().is_empty() {
        return Err(ProviderRepositoryError::InvalidInput { field: "name" });
    }
    if settings.base_url.trim().is_empty() {
        return Err(ProviderRepositoryError::InvalidInput { field: "baseUrl" });
    }
    let mut seen = std::collections::BTreeSet::new();
    for model in &settings.models {
        if model.model_id.trim().is_empty() || !seen.insert(model.model_id.as_str()) {
            return Err(ProviderRepositoryError::InvalidInput { field: "models" });
        }
        if let Some(capabilities) = model.capabilities
            && capabilities.validate().is_err()
        {
            return Err(ProviderRepositoryError::InvalidInput {
                field: "models.capabilities",
            });
        }
    }
    Ok(())
}

#[async_trait]
impl ProviderRepository for FileProviderRepository {
    async fn list_profiles(&self) -> Result<Vec<ProviderProfile>, ProviderRepositoryError> {
        let config = self.read().await?;
        Ok(config
            .providers
            .iter()
            .map(|(id, stored)| stored.profile(id))
            .collect())
    }

    async fn get_profile(
        &self,
        id: &str,
    ) -> Result<Option<ProviderProfile>, ProviderRepositoryError> {
        let config = self.read().await?;
        Ok(config.providers.get(id).map(|stored| stored.profile(id)))
    }

    async fn load_runtime(
        &self,
        id: &str,
    ) -> Result<Option<ProviderRuntimeConfig>, ProviderRepositoryError> {
        let config = self.read().await?;
        let Some(stored) = config.providers.get(id) else {
            return Ok(None);
        };
        let credential = stored
            .credential()
            .ok_or_else(|| ProviderRepositoryError::MissingCredential { id: id.to_string() })?;
        Ok(Some(ProviderRuntimeConfig {
            id: id.to_string(),
            settings: stored.settings.clone(),
            credential,
        }))
    }

    async fn create(
        &self,
        id: &str,
        input: ProviderInput,
    ) -> Result<ProviderProfile, ProviderRepositoryError> {
        validate(id, &input.settings)?;
        let _guard = self.write.lock().await;
        let mut config = self.read().await?;
        if config.providers.contains_key(id) {
            return Err(ProviderRepositoryError::AlreadyExists { id: id.to_string() });
        }
        let stored = StoredProvider {
            settings: input.settings,
            api_key: input.api_key,
        };
        let profile = stored.profile(id);
        config.providers.insert(id.to_string(), stored);
        self.write(&config).await?;
        Ok(profile)
    }

    async fn update(
        &self,
        id: &str,
        input: ProviderInput,
    ) -> Result<ProviderProfile, ProviderRepositoryError> {
        validate(id, &input.settings)?;
        let _guard = self.write.lock().await;
        let mut config = self.read().await?;
        let Some(existing) = config.providers.get(id) else {
            return Err(ProviderRepositoryError::NotFound { id: id.to_string() });
        };
        // 更新时没有给出 key，表示保留原来的 key。
        let api_key = input.api_key.or_else(|| existing.api_key.clone());
        let stored = StoredProvider {
            settings: input.settings,
            api_key,
        };
        let profile = stored.profile(id);
        config.providers.insert(id.to_string(), stored);
        self.write(&config).await?;
        Ok(profile)
    }

    async fn delete(&self, id: &str) -> Result<(), ProviderRepositoryError> {
        let _guard = self.write.lock().await;
        let mut config = self.read().await?;
        if config.providers.remove(id).is_none() {
            return Err(ProviderRepositoryError::NotFound { id: id.to_string() });
        }
        self.write(&config).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings(name: &str) -> ProviderSettings {
        ProviderSettings {
            name: name.to_string(),
            base_url: "https://api.example.com/v1".to_string(),
            env_key: None,
            http_headers: BTreeMap::new(),
            query_params: BTreeMap::new(),
            request_max_retries: None,
            stream_idle_timeout_ms: None,
            models: Vec::new(),
            enabled: true,
        }
    }

    #[test]
    fn model_refs_split_on_the_first_separator() {
        let parsed = ModelRef::parse("openrouter/anthropic/claude").unwrap();
        assert_eq!(parsed.provider_id, "openrouter");
        assert_eq!(parsed.model_id, "anthropic/claude");
        assert_eq!(parsed.to_ref_string(), "openrouter/anthropic/claude");
        assert!(ModelRef::parse("no-separator").is_none());
        assert!(ModelRef::parse("/model").is_none());
    }

    #[tokio::test]
    async fn keeps_the_key_on_update_and_preserves_other_config_keys() {
        let directory = tempfile::TempDir::new().unwrap();
        let path = directory.path().join("config.json");
        std::fs::write(&path, r#"{"theme":"dark"}"#).unwrap();
        let repository = FileProviderRepository::new(&path);

        repository
            .create(
                "deepseek",
                ProviderInput {
                    settings: settings("DeepSeek"),
                    api_key: Some("sk-test".to_string()),
                },
            )
            .await
            .unwrap();
        let updated = repository
            .update(
                "deepseek",
                ProviderInput {
                    settings: settings("DeepSeek 2"),
                    api_key: None,
                },
            )
            .await
            .unwrap();

        assert!(updated.has_api_key);
        let runtime = repository.load_runtime("deepseek").await.unwrap().unwrap();
        assert_eq!(runtime.credential.expose(), "sk-test");
        let stored: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(stored["theme"], "dark");
        assert_eq!(stored["providers"]["deepseek"]["name"], "DeepSeek 2");
    }

    #[tokio::test]
    async fn a_provider_without_any_key_reports_the_missing_credential() {
        let directory = tempfile::TempDir::new().unwrap();
        let repository = FileProviderRepository::new(directory.path().join("config.json"));
        repository
            .create(
                "custom",
                ProviderInput {
                    settings: settings("Custom"),
                    api_key: None,
                },
            )
            .await
            .unwrap();

        assert!(matches!(
            repository.load_runtime("custom").await,
            Err(ProviderRepositoryError::MissingCredential { .. })
        ));
    }

    #[tokio::test]
    async fn rejects_provider_ids_that_contain_the_separator() {
        let directory = tempfile::TempDir::new().unwrap();
        let repository = FileProviderRepository::new(directory.path().join("config.json"));
        assert!(matches!(
            repository
                .create(
                    "a/b",
                    ProviderInput {
                        settings: settings("X"),
                        api_key: None,
                    },
                )
                .await,
            Err(ProviderRepositoryError::InvalidInput { field: "id" })
        ));
    }
}
