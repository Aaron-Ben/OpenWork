use openwork_providers::{
    test_provider, ProviderConfig, ProviderIndex, ProviderInput, ProviderPreset, ProviderStore,
    TestResult, BUILTIN_PRESETS,
};

#[tauri::command]
pub async fn provider_list(
    store: tauri::State<'_, ProviderStore>,
) -> Result<ProviderIndex, String> {
    store.index().await.map_err(|error| error.to_string())
}

#[tauri::command]
pub fn provider_presets() -> Vec<ProviderPreset> {
    BUILTIN_PRESETS.to_vec()
}

#[tauri::command]
pub async fn provider_create(
    store: tauri::State<'_, ProviderStore>,
    input: ProviderInput,
) -> Result<ProviderConfig, String> {
    store.add(input).await.map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn provider_update(
    store: tauri::State<'_, ProviderStore>,
    id: String,
    input: ProviderInput,
) -> Result<ProviderConfig, String> {
    store
        .update(&id, input)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn provider_delete(
    store: tauri::State<'_, ProviderStore>,
    id: String,
) -> Result<(), String> {
    store.delete(&id).await.map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn provider_activate(
    store: tauri::State<'_, ProviderStore>,
    id: String,
) -> Result<(), String> {
    store.activate(&id).await.map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn provider_test(config: ProviderConfig, model: String) -> TestResult {
    test_provider(&config, &model).await
}
