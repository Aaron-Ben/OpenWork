use std::sync::Arc;

use openwork_core::{
    skills::{SkillDetail, SkillDiscovery},
    OpenWorkCore as OpenWorkCoreService,
};

type OpenWorkCore = Arc<OpenWorkCoreService>;

use crate::CommandError;

#[tauri::command]
pub async fn list_skills(
    core: tauri::State<'_, OpenWorkCore>,
) -> Result<SkillDiscovery, CommandError> {
    core.list_skills().await.map_err(CommandError::from)
}

#[tauri::command]
pub async fn set_skill_disabled(
    core: tauri::State<'_, OpenWorkCore>,
    name: String,
    disabled: bool,
) -> Result<SkillDiscovery, CommandError> {
    core.set_skill_disabled(&name, disabled)
        .await
        .map_err(CommandError::from)
}

#[tauri::command]
pub async fn read_skill(
    core: tauri::State<'_, OpenWorkCore>,
    path: String,
) -> Result<SkillDetail, CommandError> {
    core.read_skill(&path).await.map_err(CommandError::from)
}
