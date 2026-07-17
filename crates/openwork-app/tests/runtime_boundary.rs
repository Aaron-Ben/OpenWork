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

#[test]
fn workspace_has_no_protocol_or_persistence_compatibility_crates() {
    let workspace_manifest = include_str!("../../../Cargo.toml");
    let app_manifest = include_str!("../Cargo.toml");
    for removed in ["openwork-protocol", "openwork-persistence"] {
        assert!(!workspace_manifest.contains(removed));
        assert!(!app_manifest.contains(removed));
    }
    let workspace = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    assert!(
        !workspace
            .join("crates/openwork-protocol/Cargo.toml")
            .exists()
    );
    assert!(
        !workspace
            .join("crates/openwork-persistence/Cargo.toml")
            .exists()
    );
}
