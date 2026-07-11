use std::path::Path;

#[test]
fn protocol_source_tree_matches_architecture_blueprint() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    assert!(!src.join("ai").exists(), "legacy src/ai must be removed");
    for path in [
        "domain/mod.rs",
        "domain/ids.rs",
        "journal/mod.rs",
        "journal/port.rs",
        "journal/types.rs",
        "approval/mod.rs",
        "capability/mod.rs",
        "capability/port.rs",
        "capability/types.rs",
        "model/mod.rs",
        "model/message.rs",
        "model/request.rs",
        "model/response.rs",
        "model/event.rs",
        "model/error.rs",
        "model/port.rs",
        "provider/mod.rs",
        "provider/driver.rs",
        "provider/profile.rs",
        "provider/repository.rs",
    ] {
        assert!(
            src.join(path).is_file(),
            "missing protocol source file: {path}"
        );
    }
}

#[test]
fn tool_responsibilities_are_split_without_legacy_crate() {
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let crates = workspace.join("crates");
    assert!(
        crates.join("openwork-capabilities").is_dir(),
        "capability catalog crate is missing"
    );
    assert!(
        crates.join("openwork-execution").is_dir(),
        "execution crate is missing"
    );
    assert!(
        !crates.join("openwork-tools").exists(),
        "legacy openwork-tools crate must be removed after the split"
    );

    let core_manifest = std::fs::read_to_string(crates.join("openwork-core/Cargo.toml")).unwrap();
    assert!(!core_manifest.contains("openwork-tools"));
    assert!(!core_manifest.contains("openwork-capabilities"));
    assert!(!core_manifest.contains("openwork-execution"));

    for legacy in ["openwork-agent", "openwork-runtime", "openwork-permissions"] {
        assert!(
            !crates.join(legacy).exists(),
            "legacy crate must be removed: {legacy}"
        );
    }

    let capabilities_manifest =
        std::fs::read_to_string(crates.join("openwork-capabilities/Cargo.toml")).unwrap();
    assert!(!capabilities_manifest.contains("openwork-execution"));

    let execution_manifest =
        std::fs::read_to_string(crates.join("openwork-execution/Cargo.toml")).unwrap();
    let dependencies = execution_manifest
        .split("[dev-dependencies]")
        .next()
        .unwrap_or(&execution_manifest);
    assert!(!dependencies.contains("openwork-capabilities"));
}
