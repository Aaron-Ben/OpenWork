use std::path::Path;

#[test]
fn persistence_is_only_a_provider_credential_compatibility_boundary_for_the_app() {
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let crate_src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");

    for path in [
        "crypto/mod.rs",
        "crypto/api_key.rs",
        "postgres/database.rs",
        "postgres/persistence.rs",
        "postgres/provider_registry/mod.rs",
        "postgres/provider_registry/record.rs",
        "postgres/provider_registry/repository.rs",
    ] {
        assert!(
            crate_src.join(path).is_file(),
            "missing provider credential compatibility source: {path}"
        );
    }

    let application =
        std::fs::read_to_string(workspace.join("crates/openwork-app/src/application.rs"))
            .expect("application composition root must be readable");
    assert!(application.contains("persistence.provider_repository()"));
    assert!(application.contains("PostgresStorage::from_pool"));
    assert!(!application.contains("persistence.session_store()"));
    assert!(!application.contains("event_journal"));
    assert!(!application.contains("trace_repository"));

    let desktop = std::fs::read_to_string(workspace.join("apps/desktop/src-tauri/src/lib.rs"))
        .expect("desktop host must be readable");
    assert!(!desktop.contains("PostgresPersistence"));
    assert!(!desktop.contains("SessionStore"));

    for removed in [
        "openwork-capabilities",
        "openwork-execution",
        "openwork-observability",
        "openwork-providers",
        "openwork-workspace",
    ] {
        assert!(
            !workspace
                .join("crates")
                .join(removed)
                .join("Cargo.toml")
                .exists(),
            "removed legacy crate still exists: {removed}"
        );
    }

    let workspace_manifest = std::fs::read_to_string(workspace.join("Cargo.toml")).unwrap();
    for removed in [
        "openwork-capabilities",
        "openwork-execution",
        "openwork-observability",
        "openwork-providers",
        "openwork-workspace",
    ] {
        assert!(
            !workspace_manifest.contains(removed),
            "workspace still references removed crate: {removed}"
        );
    }
}
