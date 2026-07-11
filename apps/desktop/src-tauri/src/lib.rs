mod commands;

use std::sync::Arc;

use commands::provider::ProviderRepositoryState;
use openwork_app::{ChatRuntime, RequestCancelRegistry};
use openwork_persistence::PostgresPersistence;
use openwork_protocol::provider::ProviderRepository;
use openwork_providers::ProviderFactory;
use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    load_development_env();

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let persistence =
                tauri::async_runtime::block_on(PostgresPersistence::connect_from_env_or_local())?;
            let provider_repository: Arc<dyn ProviderRepository> =
                Arc::new(persistence.provider_repository());
            let session_store = persistence.session_store();
            let provider_factory = ProviderFactory::default();
            app.manage(ChatRuntime::new(
                Arc::clone(&provider_repository),
                session_store.clone(),
                provider_factory.clone(),
            ));
            app.manage(ProviderRepositoryState(provider_repository));
            app.manage(provider_factory);
            app.manage(session_store);
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

#[cfg(debug_assertions)]
fn load_development_env() {
    // `pnpm tauri dev` starts Cargo below `apps/desktop`; dotenvy searches parent
    // directories, so it finds the repository-root `.env`. Existing process
    // variables keep precedence over values from the file.
    let _ = dotenvy::dotenv();
}

#[cfg(not(debug_assertions))]
fn load_development_env() {}

#[cfg(test)]
mod tests {
    use crate::commands::provider::BUILTIN_PRESETS;

    #[test]
    fn builtin_presets_exclude_local_models() {
        let ids: Vec<&str> = BUILTIN_PRESETS.iter().map(|preset| preset.id).collect();
        assert!(!ids.contains(&"ollama"));
        assert!(!ids.contains(&"lmstudio"));
        assert!(!ids.contains(&"official"));
        assert!(!ids.contains(&"custom"));
    }

    #[test]
    fn provider_ui_excludes_custom_creation_path() {
        let ui_src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../src");
        for path in ["components/ProviderFormModal.tsx", "type/providers.ts"] {
            let source = std::fs::read_to_string(ui_src.join(path)).unwrap();
            assert!(!source.contains("openai_compatible"), "{path}");
            assert!(!source.contains("custom"), "{path}");
            assert!(!source.contains("Custom"), "{path}");
        }
    }
}
