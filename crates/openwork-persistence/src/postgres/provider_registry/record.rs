use sqlx::FromRow;

#[derive(Debug, Clone, FromRow)]
pub(super) struct ProviderProfileRecord {
    pub id: String,
    pub name: String,
    pub base_url: String,
    pub driver_code: String,
    pub enabled: bool,
}

#[derive(Debug, Clone, FromRow)]
pub(super) struct ProviderRuntimeRecord {
    pub id: String,
    pub name: String,
    pub base_url: String,
    pub api_key: String,
    pub driver_code: String,
    pub enabled: bool,
    pub adapter_options_json: Option<serde_json::Value>,
}

#[derive(Debug, Clone, FromRow)]
pub(super) struct ProviderModelRecord {
    pub provider_id: String,
    pub model_id: String,
    pub display_name: Option<String>,
    pub model_tier: String,
    pub enabled: bool,
}
