use std::{fs, path::Path};

use serde::Deserialize;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Baseline {
    version: u32,
    status: String,
    public_seams: Vec<String>,
    behavior_coverage: Vec<BehaviorCoverage>,
    reset: ResetInventory,
}

#[derive(Deserialize)]
struct BehaviorCoverage {
    behavior: String,
    #[serde(rename = "testFile")]
    test_file: String,
    #[serde(rename = "testName")]
    test_name: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ResetInventory {
    dry_run_only: bool,
    postgres_tables: Vec<String>,
    redis_namespaces: Vec<String>,
    filesystem: FilesystemInventory,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct FilesystemInventory {
    preserve: Vec<String>,
    review_before_delete: Vec<String>,
    regenerate: Vec<String>,
    retire_after_cutover: Vec<String>,
}

#[test]
fn r0_artifact_remains_a_read_only_record_of_the_pre_cutover_system() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/r0-baseline.json");
    let baseline: Baseline = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();

    assert_eq!(baseline.version, 1);
    assert_eq!(baseline.status, "frozen-current-state");
    assert!(baseline.reset.dry_run_only);
    assert_eq!(baseline.public_seams.len(), 4);
    assert!(!baseline.behavior_coverage.is_empty());
    assert!(baseline.behavior_coverage.iter().all(|coverage| {
        !coverage.behavior.is_empty()
            && !coverage.test_file.is_empty()
            && !coverage.test_name.is_empty()
    }));
    assert!(!baseline.reset.postgres_tables.is_empty());
    assert!(!baseline.reset.redis_namespaces.is_empty());
    assert!(
        baseline
            .reset
            .filesystem
            .preserve
            .iter()
            .any(|path| path.contains("work"))
    );
    assert!(!baseline.reset.filesystem.review_before_delete.is_empty());
    assert!(!baseline.reset.filesystem.regenerate.is_empty());
    assert!(!baseline.reset.filesystem.retire_after_cutover.is_empty());
}
