use std::path::Path;

#[test]
fn persistence_source_tree_matches_model_provider_design() {
    let crate_src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    for path in ["crypto/mod.rs", "crypto/api_key.rs"] {
        assert!(
            crate_src.join(path).is_file(),
            "missing persistence source file: {path}"
        );
    }
    let src = crate_src.join("postgres");
    for path in [
        "persistence.rs",
        "migrations/mod.rs",
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
}
