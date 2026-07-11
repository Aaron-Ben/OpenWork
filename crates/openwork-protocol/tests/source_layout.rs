use std::path::Path;

#[test]
fn protocol_source_tree_matches_architecture_blueprint() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    assert!(!src.join("ai").exists(), "legacy src/ai must be removed");
    for path in [
        "domain/mod.rs",
        "domain/ids.rs",
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
