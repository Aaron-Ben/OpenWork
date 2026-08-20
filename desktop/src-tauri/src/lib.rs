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
            let collab = tauri::async_runtime::block_on(
                collab_client::CollabDaemonClient::discover_or_start(),
            )?;
            event_bridge::spawn_collab_event_bridge(app.handle().clone(), collab.clone());
            app.manage(collab);
            app.manage(core);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::collab::collab_status,
            commands::collab::collab_agent_list,
            commands::collab::collab_agent_create,
            commands::collab::collab_agent_update,
            commands::collab::collab_room_list,
            commands::collab::collab_room_create,
            commands::collab::collab_room_add_member,
            commands::collab::collab_room_remove_member,
            commands::collab::collab_room_set_muted,
            commands::collab::collab_message_send,
            commands::collab::collab_message_page,
            commands::collab::collab_room_mark_read,
            commands::collab::collab_permission_list,
            commands::collab::collab_permission_reply,
            commands::collab::collab_permission_abort,
            commands::collab::collab_log_list,
            commands::collab::collab_board_list,
            commands::collab::collab_board_create,
            commands::collab::collab_board_column_create,
            commands::collab::collab_card_create,
            commands::collab::collab_card_move,
            commands::collab::collab_card_release_claim,
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
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
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
