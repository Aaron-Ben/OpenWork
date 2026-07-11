use std::path::Path;

#[test]
fn persistence_source_tree_matches_model_provider_design() {
    let crate_src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    for path in [
        "bin/openwork-migrate.rs",
        "crypto/mod.rs",
        "crypto/api_key.rs",
        "session/mod.rs",
        "session/store.rs",
        "session/types.rs",
    ] {
        assert!(
            crate_src.join(path).is_file(),
            "missing persistence source file: {path}"
        );
    }
    let src = crate_src.join("postgres");
    for path in [
        "database.rs",
        "persistence.rs",
        "migrations/mod.rs",
        "migrations/runner.rs",
        "migrations/drop_legacy_sessions.rs",
        "migrations/recorded_events.rs",
        "migrations/schema_infrastructure.rs",
        "event_journal/mod.rs",
        "event_journal/record.rs",
        "event_journal/repository.rs",
        "migrations/provider_registry.rs",
        "provider_registry/mod.rs",
        "provider_registry/record.rs",
        "provider_registry/repository.rs",
    ] {
        assert!(
            src.join(path).is_file(),
            "missing persistence source file: {path}"
        );
    }
    for legacy in [
        "provider_migrations.rs",
        "provider_records.rs",
        "provider_repository.rs",
    ] {
        assert!(
            !src.join(legacy).exists(),
            "legacy postgres module remains: {legacy}"
        );
    }

    let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let application =
        std::fs::read_to_string(workspace.join("crates/openwork-app/src/application.rs"))
            .expect("application composition root must be readable");
    assert!(application.contains("persistence.session_store()"));
    assert!(application.contains("PostgresPersistence::connect"));

    let desktop = std::fs::read_to_string(workspace.join("apps/desktop/src-tauri/src/lib.rs"))
        .expect("desktop host must be readable");
    assert!(!desktop.contains("PostgresPersistence"));
    assert!(!desktop.contains("SessionStore"));

    let chat = std::fs::read_to_string(workspace.join("crates/openwork-app/src/chat.rs"))
        .expect("chat runtime must be readable");
    assert!(!chat.contains("append_llm_event"));
    assert!(!chat.contains("tokio::spawn"));
    assert!(chat.contains("start_turn"));
    assert!(chat.contains("finish_turn"));

    assert!(!workspace.join("crates/openwork-session").exists());
    assert!(!workspace.join("crates/openwork-db-macros").exists());
    assert!(!workspace.join("crates/openwork-database").exists());

    let workspace_manifest = std::fs::read_to_string(workspace.join("Cargo.toml")).unwrap();
    let persistence_manifest =
        std::fs::read_to_string(workspace.join("crates/openwork-persistence/Cargo.toml")).unwrap();
    assert!(!workspace_manifest.contains("openwork-database"));
    assert!(!persistence_manifest.contains("openwork-database"));
}
