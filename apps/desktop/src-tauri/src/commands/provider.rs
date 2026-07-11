use openwork_app::{
    OpenWorkApplication, ProviderIndex, ProviderInput, ProviderPreset, ProviderProfile,
    ProviderTestResult,
};

use crate::CommandError;

#[tauri::command]
pub async fn provider_list(
    application: tauri::State<'_, OpenWorkApplication>,
) -> Result<ProviderIndex, CommandError> {
    application
        .providers()
        .list()
        .await
        .map_err(CommandError::from)
}

#[tauri::command]
pub fn provider_presets(application: tauri::State<'_, OpenWorkApplication>) -> Vec<ProviderPreset> {
    application.providers().presets()
}

#[tauri::command]
pub async fn provider_create(
    application: tauri::State<'_, OpenWorkApplication>,
    input: ProviderInput,
) -> Result<ProviderProfile, CommandError> {
    application
        .providers()
        .create(input)
        .await
        .map_err(CommandError::from)
}

#[tauri::command]
pub async fn provider_update(
    application: tauri::State<'_, OpenWorkApplication>,
    id: String,
    input: ProviderInput,
) -> Result<ProviderProfile, CommandError> {
    application
        .providers()
        .update(&id, input)
        .await
        .map_err(CommandError::from)
}

#[tauri::command]
pub async fn provider_delete(
    application: tauri::State<'_, OpenWorkApplication>,
    id: String,
) -> Result<(), CommandError> {
    application
        .providers()
        .delete(&id)
        .await
        .map_err(CommandError::from)
}

#[tauri::command]
pub async fn provider_activate(
    application: tauri::State<'_, OpenWorkApplication>,
    id: String,
) -> Result<(), CommandError> {
    application
        .providers()
        .activate(&id)
        .await
        .map_err(CommandError::from)
}

#[tauri::command]
pub async fn provider_test(
    application: tauri::State<'_, OpenWorkApplication>,
    id: Option<String>,
    input: Option<ProviderInput>,
    model: String,
) -> Result<ProviderTestResult, CommandError> {
    application
        .providers()
        .test(id, input, &model)
        .await
        .map_err(CommandError::from)
}
