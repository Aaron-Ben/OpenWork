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
