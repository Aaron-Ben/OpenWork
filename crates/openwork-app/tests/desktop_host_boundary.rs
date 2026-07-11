use std::{fs, path::Path};

fn repository_root() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

#[test]
fn desktop_host_depends_on_application_boundary_instead_of_internal_adapters() {
    let root = repository_root();
    let desktop = root.join("apps/desktop/src-tauri");
    let manifest = fs::read_to_string(desktop.join("Cargo.toml")).unwrap();

    for forbidden in [
        "openwork-persistence",
        "openwork-providers",
        "openwork-execution",
        "openwork-workspace",
    ] {
        assert!(
            !manifest.contains(forbidden),
            "desktop host must not depend on {forbidden}"
        );
    }

    let commands = ["chat.rs", "provider.rs", "session.rs"]
        .into_iter()
        .map(|file| fs::read_to_string(desktop.join("src/commands").join(file)).unwrap())
        .collect::<Vec<_>>()
        .join("\n");

    for forbidden in [
        "SessionStore",
        "ProviderFactory",
        "ProviderRepositoryState",
        "PostgresPersistence",
    ] {
        assert!(
            !commands.contains(forbidden),
            "desktop commands must not reference {forbidden}"
        );
    }
    assert!(
        !commands.contains(", String>"),
        "desktop commands must return structured CommandError values"
    );
}

#[test]
fn desktop_host_manages_one_application_root_and_preserves_command_names() {
    let root = repository_root();
    let desktop = root.join("apps/desktop/src-tauri");
    let host = fs::read_to_string(desktop.join("src/lib.rs")).unwrap();

    assert!(host.contains("OpenWorkApplication"));
    assert_eq!(
        host.matches("app.manage(").count(),
        1,
        "desktop should manage only the application root"
    );

    for command in [
        "provider_list",
        "provider_presets",
        "provider_create",
        "provider_update",
        "provider_delete",
        "provider_activate",
        "provider_test",
        "session_list",
        "session_create",
        "session_load",
        "session_delete",
        "session_rename",
        "chat_generate_stream",
        "resolve_approval",
        "chat_abort",
    ] {
        assert!(host.contains(command), "missing Tauri command {command}");
    }
}
