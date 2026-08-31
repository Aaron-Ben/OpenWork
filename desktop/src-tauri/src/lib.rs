mod collab_client;
mod collab_event_bridge;
mod commands;
mod error;
mod event_bridge;

use openwork_core::{OpenWorkCore, OpenWorkCoreConfig};
use tauri::Manager;

pub use collab_client::{CollabClientError, CollabDaemonClient};
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
            collab_event_bridge::spawn(app.handle().clone(), collab.subscribe_invalidations());
            app.manage(collab);
            app.manage(core);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::collab::collab_status,
            commands::collab::collab_agent_list,
            commands::collab::collab_agent_create,
            commands::collab::collab_agent_update,
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
            commands::collab::collab_board_update,
            commands::collab::collab_board_delete,
            commands::collab::collab_board_column_create,
            commands::collab::collab_board_column_update,
            commands::collab::collab_board_column_move,
            commands::collab::collab_board_column_delete,
            commands::collab::collab_card_assign,
            commands::collab::collab_card_delete,
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
    let (collab, exit_code) =
        run_return_with_managed_state::<_, collab_client::CollabDaemonClient>(app);
    tauri::async_runtime::block_on(collab.shutdown());
    std::process::exit(exit_code);
}

fn run_return_with_managed_state<R, T>(app: tauri::App<R>) -> (T, i32)
where
    R: tauri::Runtime,
    T: Clone + Send + Sync + 'static,
{
    let handle = app.handle().clone();
    let exit_code = app.run_return(|_, _| {});
    let state = handle.state::<T>().inner().clone();
    (state, exit_code)
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

#[cfg(test)]
mod tests {
    use tauri::Manager;

    use super::run_return_with_managed_state;

    #[derive(Clone)]
    struct SetupManagedState;

    #[test]
    fn managed_state_is_read_after_setup_runs() {
        let (setup_tx, setup_rx) = std::sync::mpsc::channel();
        let app = tauri::test::mock_builder()
            .setup(move |app| {
                assert!(app.manage(SetupManagedState));
                setup_tx.send(()).unwrap();
                Ok(())
            })
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .unwrap();
        let window = tauri::WebviewWindowBuilder::new(&app, "lifecycle", Default::default())
            .build()
            .unwrap();
        let closer = std::thread::spawn(move || {
            setup_rx.recv().unwrap();
            window.close().unwrap();
        });

        let (_, exit_code) = run_return_with_managed_state::<_, SetupManagedState>(app);

        closer.join().unwrap();
        assert_eq!(exit_code, 0);
    }
}
