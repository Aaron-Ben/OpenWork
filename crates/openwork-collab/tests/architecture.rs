use std::path::{Path, PathBuf};

#[test]
fn p0_has_one_local_opencode_path_and_no_legacy_collaboration_dependencies() {
    let crate_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let manifest = std::fs::read_to_string(crate_root.join("Cargo.toml")).unwrap();
    for dependency in [
        "rmcp",
        "openwork-core",
        "openwork-credentials",
        "openwork-models",
    ] {
        assert!(
            !manifest.contains(dependency),
            "legacy collaboration dependency survived: {dependency}"
        );
    }

    let computer = source_text(&crate_root.join("src/computer"));
    assert!(!computer.contains("sqlx::"));
    assert!(!computer.contains("redis::"));
    let shim = std::fs::read_to_string(crate_root.join("src/computer/shim.rs")).unwrap();
    assert_eq!(computer.matches("/runtime/cli").count(), 1);
    assert!(shim.contains("/runtime/cli"));
    let server = source_text(&crate_root.join("src/server"));
    assert!(!server.contains("Command::new"));

    let source = source_text(&crate_root.join("src"));
    for excluded in [
        "target_os = \"windows\"",
        "target_os = \"linux\"",
        "opencode serve",
        "ClaudeAdapter",
        "CodexAdapter",
        "pairing_code",
    ] {
        assert!(
            !source.contains(excluded),
            "excluded P0 path survived: {excluded}"
        );
    }
}

#[test]
fn r1_server_business_modules_own_persistence_without_a_universal_store() {
    let crate_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let server_root = crate_root.join("src/server");
    assert!(!server_root.join("storage.rs").exists());

    let server = source_text(&server_root);
    assert!(!server.contains("CollaborationStore"));
    for adapter in ["control.rs", "runtime.rs", "scheduler.rs"] {
        let source = std::fs::read_to_string(server_root.join(adapter)).unwrap();
        assert!(
            !source.contains("sqlx::query"),
            "transport/orchestration adapter owns business SQL: {adapter}"
        );
    }
}

fn source_text(root: &Path) -> String {
    let mut text = String::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(path) = pending.pop() {
        if path.is_dir() {
            for entry in std::fs::read_dir(path).unwrap() {
                pending.push(entry.unwrap().path());
            }
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            text.push_str(&std::fs::read_to_string(path).unwrap());
        }
    }
    text
}
