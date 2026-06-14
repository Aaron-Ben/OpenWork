use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use anvil_core::ai::{GenerateRequest, Message, Role};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::provider_config::{ProviderConfig, ProviderInput, build_provider};

/// 磁盘上的 provider 索引(providers.json 的根结构)。
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
    #[error("failed to persist provider store: {0}")]
    Io(#[from] std::io::Error),
    #[error("failed to serialize provider store: {0}")]
    Serialize(#[from] serde_json::Error),
}

/// Provider 配置仓库:内存状态(Mutex 保护)+ 原子持久化。
pub struct ProviderStore {
    path: PathBuf,
    state: Mutex<ProviderIndex>,
}

impl ProviderStore {
    /// 打开仓库;文件不存在或损坏则返回空状态(不 panic)。
    pub fn open(path: impl Into<PathBuf>) -> Self {
        let path = path.into();
        let state = load_index(&path).unwrap_or_default();
        Self {
            path,
            state: Mutex::new(state),
        }
    }

    pub fn index(&self) -> ProviderIndex {
        self.state
            .lock()
            .expect("provider store mutex poisoned")
            .clone()
    }

    pub fn list(&self) -> Vec<ProviderConfig> {
        self.index().providers
    }

    pub fn get(&self, id: &str) -> Option<ProviderConfig> {
        self.index().providers.into_iter().find(|p| p.id == id)
    }

    pub fn active(&self) -> Option<ProviderConfig> {
        let index = self.index();
        let active_id = index.active_id.as_ref()?;
        index.providers.into_iter().find(|p| &p.id == active_id)
    }

    pub fn add(&self, input: ProviderInput) -> Result<ProviderConfig, StoreError> {
        validate_input(&input)?;
        let config = ProviderConfig::new(generate_id(), input);
        let mut state = self.state.lock().expect("provider store mutex poisoned");
        state.providers.push(config.clone());
        self.persist(&state)?;
        Ok(config)
    }

    pub fn update(&self, id: &str, input: ProviderInput) -> Result<ProviderConfig, StoreError> {
        validate_input(&input)?;
        let mut state = self.state.lock().expect("provider store mutex poisoned");
        let config = state
            .providers
            .iter_mut()
            .find(|p| p.id == id)
            .ok_or_else(|| StoreError::NotFound { id: id.to_string() })?;
        config.data = input;
        let updated = config.clone();
        self.persist(&state)?;
        Ok(updated)
    }

    pub fn delete(&self, id: &str) -> Result<(), StoreError> {
        let mut state = self.state.lock().expect("provider store mutex poisoned");
        if state.active_id.as_deref() == Some(id) {
            return Err(StoreError::CannotDeleteActive { id: id.to_string() });
        }
        let before = state.providers.len();
        state.providers.retain(|p| p.id != id);
        if state.providers.len() == before {
            return Err(StoreError::NotFound { id: id.to_string() });
        }
        self.persist(&state)
    }

    pub fn activate(&self, id: &str) -> Result<(), StoreError> {
        let mut state = self.state.lock().expect("provider store mutex poisoned");
        if !state.providers.iter().any(|p| p.id == id) {
            return Err(StoreError::NotFound { id: id.to_string() });
        }
        state.active_id = Some(id.to_string());
        self.persist(&state)
    }

    pub fn clear_active(&self) -> Result<(), StoreError> {
        let mut state = self.state.lock().expect("provider store mutex poisoned");
        state.active_id = None;
        self.persist(&state)
    }

    fn persist(&self, state: &ProviderIndex) -> Result<(), StoreError> {
        write_index_atomic(&self.path, state)
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

fn generate_id() -> String {
    format!("prov-{}", Uuid::new_v4().simple())
}

fn load_index(path: &Path) -> Option<ProviderIndex> {
    let contents = fs::read_to_string(path).ok()?;
    serde_json::from_str(&contents).ok()
}

fn write_index_atomic(path: &Path, index: &ProviderIndex) -> Result<(), StoreError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let json = serde_json::to_string_pretty(index)?;
    let tmp_path = PathBuf::from(format!("{}.tmp", path.display()));

    {
        let mut file = fs::File::create(&tmp_path)?;
        file.write_all(json.as_bytes())?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            file.set_permissions(fs::Permissions::from_mode(0o600))?;
        }
    }

    fs::rename(&tmp_path, path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider_config::ProviderKind;

    fn temp_path() -> PathBuf {
        let mut path = std::env::temp_dir();
        path.push(format!("anvil-test-{}.json", Uuid::new_v4().simple()));
        path
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

    #[test]
    fn open_missing_file_is_empty() {
        let path = temp_path();
        let store = ProviderStore::open(&path);
        assert!(store.list().is_empty());
        assert!(store.active().is_none());
    }

    #[test]
    fn add_persists_and_assigns_id() {
        let path = temp_path();
        let store = ProviderStore::open(&path);
        let config = store.add(sample_input()).unwrap();

        assert!(config.id.starts_with("prov-"));
        assert_eq!(store.list().len(), 1);

        // 重新打开验证持久化
        let store2 = ProviderStore::open(&path);
        assert_eq!(store2.list().len(), 1);
        assert_eq!(store2.list()[0].id, config.id);

        let _ = fs::remove_file(&path);
    }

    #[test]
    fn update_replaces_fields() {
        let path = temp_path();
        let store = ProviderStore::open(&path);
        let config = store.add(sample_input()).unwrap();

        let mut input = sample_input();
        input.name = "Renamed".to_string();
        let updated = store.update(&config.id, input).unwrap();

        assert_eq!(updated.data.name, "Renamed");
        assert_eq!(store.list()[0].data.name, "Renamed");

        let _ = fs::remove_file(&path);
    }

    #[test]
    fn delete_refuses_active_provider() {
        let path = temp_path();
        let store = ProviderStore::open(&path);
        let config = store.add(sample_input()).unwrap();
        store.activate(&config.id).unwrap();

        let error = store.delete(&config.id).unwrap_err();
        assert!(matches!(error, StoreError::CannotDeleteActive { .. }));

        let _ = fs::remove_file(&path);
    }

    #[test]
    fn delete_removes_non_active_provider() {
        let path = temp_path();
        let store = ProviderStore::open(&path);
        let first = store.add(sample_input()).unwrap();
        let second = store.add(sample_input()).unwrap();
        store.activate(&first.id).unwrap();

        store.delete(&second.id).unwrap();
        assert_eq!(store.list().len(), 1);
        assert_eq!(store.list()[0].id, first.id);

        let _ = fs::remove_file(&path);
    }

    #[test]
    fn add_rejects_empty_fields() {
        let path = temp_path();
        let store = ProviderStore::open(&path);

        let mut empty_name = sample_input();
        empty_name.name = "  ".to_string();
        assert!(matches!(
            store.add(empty_name).unwrap_err(),
            StoreError::EmptyName
        ));

        let mut empty_url = sample_input();
        empty_url.base_url = String::new();
        assert!(matches!(
            store.add(empty_url).unwrap_err(),
            StoreError::EmptyBaseUrl
        ));

        assert!(store.list().is_empty());
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn activate_sets_active_id() {
        let path = temp_path();
        let store = ProviderStore::open(&path);
        let config = store.add(sample_input()).unwrap();

        store.activate(&config.id).unwrap();
        assert_eq!(store.index().active_id, Some(config.id.clone()));
        assert_eq!(store.active().map(|c| c.id), Some(config.id));

        let _ = fs::remove_file(&path);
    }
}
