mod commands;
mod error;
mod event_bridge;

use openwork_core::{OpenWorkCore, OpenWorkCoreConfig};
use tauri::Manager;

pub use error::{CommandError, CommandErrorCode};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    load_development_env();

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let core = tauri::async_runtime::block_on(OpenWorkCore::bootstrap(
                OpenWorkCoreConfig::from_env_or_local(),
            ))?;
            event_bridge::spawn_session_update_bridge(
                app.handle().clone(),
                core.subscribe_updates(),
            );
            app.manage(core);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::provider::provider_list,
            commands::provider::provider_presets,
            commands::provider::provider_create,
            commands::provider::provider_update,
            commands::provider::provider_delete,
            commands::provider::provider_test,
            commands::runtime::runtime_session_list,
            commands::runtime::runtime_session_create,
            commands::runtime::runtime_session_load,
            commands::runtime::runtime_context_window_inspect,
            commands::runtime::runtime_session_compact,
            commands::runtime::runtime_session_rename,
            commands::runtime::runtime_session_delete,
            commands::runtime::runtime_turn_start,
            commands::runtime::runtime_turn_cancel,
            commands::runtime::runtime_file_changes_undo,
            commands::runtime::runtime_file_changes_reapply,
            commands::runtime::runtime_permission_resolve,
            commands::runtime::runtime_session_snapshot,
            commands::runtime::runtime_update_replay,
            commands::runtime::runtime_trace_list,
            commands::runtime::runtime_trace_get,
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
