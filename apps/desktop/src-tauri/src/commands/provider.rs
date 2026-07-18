use openwork_core::{
    OpenWorkCore, ProviderIndex, ProviderInput, ProviderPreset, ProviderProfile, ProviderTestResult,
};

use crate::CommandError;

#[tauri::command]
pub async fn provider_list(
    core: tauri::State<'_, OpenWorkCore>,
) -> Result<ProviderIndex, CommandError> {
    core.list_providers().await.map_err(CommandError::from)
}

#[tauri::command]
pub fn provider_presets(core: tauri::State<'_, OpenWorkCore>) -> Vec<ProviderPreset> {
    core.provider_presets()
}

#[tauri::command]
pub async fn provider_create(
    core: tauri::State<'_, OpenWorkCore>,
    input: ProviderInput,
) -> Result<ProviderProfile, CommandError> {
    core.create_provider(input)
        .await
        .map_err(CommandError::from)
}

#[tauri::command]
pub async fn provider_update(
    core: tauri::State<'_, OpenWorkCore>,
    id: String,
    input: ProviderInput,
) -> Result<ProviderProfile, CommandError> {
    core.update_provider(&id, input)
        .await
        .map_err(CommandError::from)
}

#[tauri::command]
pub async fn provider_delete(
    core: tauri::State<'_, OpenWorkCore>,
    id: String,
) -> Result<(), CommandError> {
    core.delete_provider(&id).await.map_err(CommandError::from)
}

#[tauri::command]
pub async fn provider_activate(
    core: tauri::State<'_, OpenWorkCore>,
    id: String,
) -> Result<(), CommandError> {
    core.activate_provider(&id)
        .await
        .map_err(CommandError::from)
}

#[tauri::command]
pub async fn provider_test(
    core: tauri::State<'_, OpenWorkCore>,
    id: Option<String>,
    input: Option<ProviderInput>,
    model: String,
) -> Result<ProviderTestResult, CommandError> {
    core.test_provider(id, input, &model)
        .await
        .map_err(CommandError::from)
}
