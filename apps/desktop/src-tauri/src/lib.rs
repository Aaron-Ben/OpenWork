mod commands;

use std::sync::Arc;

use commands::provider::ProviderRepositoryState;
use openwork_persistence::PostgresPersistence;
use openwork_protocol::provider::ProviderRepository;
use openwork_providers::ProviderFactory;
use openwork_runtime::{ApprovalBridge, ChatRuntime, RequestCancelRegistry};
use openwork_session::SessionStore;
use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let persistence =
                tauri::async_runtime::block_on(PostgresPersistence::connect_from_env_or_local())?;
            let provider_repository: Arc<dyn ProviderRepository> =
                Arc::new(persistence.provider_repository());
            let session_store =
                tauri::async_runtime::block_on(SessionStore::connect_from_env_or_local())?;
            let provider_factory = ProviderFactory::default();
            app.manage(ChatRuntime::new(
                Arc::clone(&provider_repository),
                session_store.clone(),
                provider_factory.clone(),
            ));
            app.manage(ProviderRepositoryState(provider_repository));
            app.manage(provider_factory);
            app.manage(session_store);
            app.manage(ApprovalBridge::new());
            app.manage(RequestCancelRegistry::default());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::provider::provider_list,
            commands::provider::provider_presets,
            commands::provider::provider_create,
            commands::provider::provider_update,
            commands::provider::provider_delete,
            commands::provider::provider_activate,
            commands::provider::provider_test,
            commands::session::session_list,
            commands::session::session_create,
            commands::session::session_load,
            commands::session::session_delete,
            commands::session::session_rename,
            commands::chat::chat_generate_stream,
            commands::chat::resolve_approval,
            commands::chat::chat_abort,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(test)]
mod tests {
    use crate::commands::provider::BUILTIN_PRESETS;

    #[test]
    fn builtin_presets_exclude_local_models() {
        let ids: Vec<&str> = BUILTIN_PRESETS.iter().map(|preset| preset.id).collect();
        assert!(!ids.contains(&"ollama"));
        assert!(!ids.contains(&"lmstudio"));
        assert!(!ids.contains(&"official"));
    }
}
