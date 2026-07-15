mod commands;
mod error;

use openwork_app::{ApplicationConfig, OpenWorkApplication};
use tauri::Manager;

pub use error::{CommandError, CommandErrorCode};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    load_development_env();

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let application = tauri::async_runtime::block_on(OpenWorkApplication::bootstrap(
                ApplicationConfig::from_env_or_local(),
            ))?;
            app.manage(application);
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
            commands::trace::trace_session,
            commands::trace::trace_turn,
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
