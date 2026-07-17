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
fn target_leaf_crates_own_models_and_tools_without_core_dependencies() {
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let crates = workspace.join("crates");
    for target in [
        "openwork-models",
        "openwork-tools",
        "openwork-agent",
        "openwork-chat-state",
    ] {
        assert!(
            crates.join(target).is_dir(),
            "target crate is missing: {target}"
        );
    }

    let models = std::fs::read_to_string(crates.join("openwork-models/Cargo.toml")).unwrap();
    assert!(!models.contains("openwork-core"));
    assert!(!models.contains("openwork-protocol"));

    let tools = std::fs::read_to_string(crates.join("openwork-tools/Cargo.toml")).unwrap();
    assert!(tools.contains("openwork-models"));
    assert!(!tools.contains("openwork-core"));
    assert!(!tools.contains("openwork-protocol"));
}
