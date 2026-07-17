use openwork_app::OpenWorkApplication;

fn assert_send_sync<T: Send + Sync>() {}

#[test]
fn application_root_is_a_send_sync_host_boundary() {
    assert_send_sync::<OpenWorkApplication>();
}

#[test]
fn desktop_exposes_runtime_commands_without_legacy_chat_commands() {
    let desktop = include_str!("../../../apps/desktop/src-tauri/src/lib.rs");
    assert!(desktop.contains("runtime_turn_start"));
    assert!(desktop.contains("runtime_session_snapshot"));
    assert!(desktop.contains("runtime_update_replay"));
    assert!(!desktop.contains("chat_generate_stream"));
    assert!(!desktop.contains("resolve_approval"));
}
