mod collab_client;
mod commands;
mod error;
mod event_bridge;

use openwork_core::{OpenWorkCore, OpenWorkCoreConfig};
use tauri::Manager;

pub use error::{CommandError, CommandErrorCode};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    load_development_env();

    let app = tauri::Builder::default()
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
            let collab = tauri::async_runtime::block_on(
                collab_client::CollabDaemonClient::discover_or_start(),
            )?;
            app.manage(collab);
            app.manage(core);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::collab::collab_status,
            commands::collab::collab_agent_list,
            commands::collab::collab_agent_create,
            commands::collab::collab_agent_agenda_set,
            commands::collab::collab_agent_archive,
            commands::collab::collab_agent_restore,
            commands::collab::collab_room_list,
            commands::collab::collab_direct_room_create,
            commands::collab::collab_group_room_create,
            commands::collab::collab_room_member_list,
            commands::collab::collab_group_member_add,
            commands::collab::collab_group_member_remove,
            commands::collab::collab_message_send,
            commands::collab::collab_message_list,
            commands::collab::collab_board_list,
            commands::collab::collab_board_create,
            commands::collab::collab_run_list,
            commands::skills::list_skills,
            commands::skills::set_skill_disabled,
            commands::skills::read_skill,
            commands::provider::provider_list,
            commands::provider::provider_presets,
            commands::provider::provider_create,
            commands::provider::provider_update,
            commands::provider::provider_delete,
            commands::provider::provider_test,
            commands::runtime::runtime_session_list,
            commands::runtime::runtime_sub_agent_list,
            commands::runtime::runtime_session_create,
            commands::runtime::runtime_session_load,
            commands::runtime::runtime_context_window_inspect,
            commands::runtime::runtime_session_compact,
            commands::runtime::runtime_session_rewind,
            commands::runtime::runtime_compaction_list,
            commands::runtime::runtime_conversation_replay,
            commands::runtime::runtime_compaction_transcript_read,
            commands::runtime::runtime_session_rename,
            commands::runtime::runtime_session_delete,
            commands::runtime::runtime_turn_start,
            commands::runtime::runtime_turn_cancel,
            commands::runtime::runtime_file_changes_undo,
            commands::runtime::runtime_file_changes_reapply,
            commands::runtime::runtime_permission_resolve,
            commands::runtime::runtime_permission_mode_set,
            commands::runtime::runtime_session_snapshot,
            commands::runtime::runtime_update_replay,
            commands::runtime::runtime_trace_list,
            commands::runtime::runtime_trace_get,
            commands::runtime::runtime_trace_get_by_id,
            commands::runtime::runtime_trace_payload_get,
            commands::runtime::runtime_trace_compactions,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application");
    let collab = app
        .state::<collab_client::CollabDaemonClient>()
        .inner()
        .clone();
    let exit_code = app.run_return(|_, _| {});
    tauri::async_runtime::block_on(collab.shutdown());
    std::process::exit(exit_code);
}

#[cfg(debug_assertions)]
fn load_development_env() {
    // `pnpm tauri dev` starts Cargo below `desktop`; dotenvy searches parent
    // directories, so it finds the repository-root `.env`. Existing process
    // variables keep precedence over values from the file.
    let _ = dotenvy::dotenv();
}

#[cfg(not(debug_assertions))]
fn load_development_env() {}
