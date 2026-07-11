use std::{fs, path::Path};

fn app_root() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).to_path_buf()
}

#[test]
fn application_has_no_model_registry_or_automatic_fallback_router() {
    let root = app_root();
    let lib = fs::read_to_string(root.join("src/lib.rs")).unwrap();
    let manifest = fs::read_to_string(root.join("Cargo.toml")).unwrap();

    assert!(
        !root.join("src/registry.rs").exists(),
        "automatic model routing registry must be removed"
    );
    for forbidden in [
        "mod registry",
        "ModelRegistry",
        "RegistryConfig",
        "FallbackRule",
    ] {
        assert!(
            !lib.contains(forbidden),
            "openwork-app must not expose automatic routing type {forbidden}"
        );
    }
    assert!(
        !manifest
            .lines()
            .any(|line| line.trim_start().starts_with("toml =")),
        "openwork-app must not keep the registry-only TOML dependency"
    );

    let workspace_manifest = fs::read_to_string(root.join("../../Cargo.toml")).unwrap();
    assert!(
        !workspace_manifest
            .lines()
            .any(|line| line.trim_start().starts_with("toml =")),
        "workspace must not keep an unused direct TOML dependency for model routing"
    );
}

#[test]
fn chat_runtime_uses_the_provider_and_model_selected_by_the_caller() {
    let chat = fs::read_to_string(app_root().join("src/chat.rs")).unwrap();

    assert!(chat.contains("load_runtime(&request.provider_id)"));
    assert!(chat.contains("request.model"));
}

#[test]
fn protocol_has_no_registry_only_model_spec() {
    let protocol = app_root().join("../openwork-protocol/src/model");
    let request = fs::read_to_string(protocol.join("request.rs")).unwrap();
    let module = fs::read_to_string(protocol.join("mod.rs")).unwrap();

    for forbidden in ["ModelSpec", "ModelCapability"] {
        assert!(
            !request.contains(forbidden),
            "protocol must not keep registry-only type {forbidden}"
        );
        assert!(
            !module.contains(forbidden),
            "protocol must not export registry-only type {forbidden}"
        );
    }
    assert!(
        request.contains("ModelCapabilities"),
        "descriptive provider capabilities are independent of automatic routing"
    );
}
