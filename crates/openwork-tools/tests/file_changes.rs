use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use openwork_tools::{
    Authorization, FileChangeArtifact, FileChangeKind, FileChangeUndoError, PermissionMode,
    PermissionProfile, ToolCallContext, ToolCallId, ToolInvocation, ToolResult, ToolSessionContext,
    ToolsetConfig, builtin_registry, reapply_file_changes, undo_file_changes,
};
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;

struct TestDirectory {
    path: PathBuf,
}

impl TestDirectory {
    fn new(label: &str) -> Self {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let id = COUNTER.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "openwork-file-change-{label}-{}-{id}",
            std::process::id()
        ));
        std::fs::create_dir_all(&path).expect("create test directory");
        Self { path }
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

fn session(root: &Path) -> ToolSessionContext {
    ToolSessionContext::local(
        root.to_path_buf(),
        PermissionProfile::from_builtin_rules(root.to_path_buf()),
    )
}

async fn call(
    toolset: &openwork_tools::FinalizedToolset,
    id: &str,
    name: &str,
    input: Value,
) -> ToolResult {
    let invocation = ToolInvocation::new(name, input);
    let permit = match toolset.authorize(&invocation, PermissionMode::AcceptEdits) {
        Authorization::Allow { permit, .. } | Authorization::Ask { permit, .. } => permit,
        Authorization::Deny { reason, .. } => panic!("test invocation denied: {reason}"),
        Authorization::Unavailable { message, .. } => {
            panic!("test invocation could not be judged: {message}")
        }
    };
    toolset
        .call(
            ToolCallContext::new(ToolCallId::new(id), CancellationToken::new()),
            invocation,
            permit,
        )
        .await
}

fn file_change(result: &ToolResult) -> FileChangeArtifact {
    FileChangeArtifact::from_result_artifact(
        result.artifacts.first().expect("file change artifact"),
    )
    .expect("valid file change artifact")
}

#[tokio::test]
async fn write_reports_complete_created_and_modified_diffs() {
    let root = TestDirectory::new("diff");
    let context = session(root.path());
    let toolset = builtin_registry()
        .finalize(&ToolsetConfig::from_names(["write"]), context)
        .expect("toolset");
    let path = root.path().join("src/main.rs");

    let created = call(
        &toolset,
        "change-create",
        "write",
        json!({"path": path, "content": "fn main() {\n    println!(\"hello\");\n}\n"}),
    )
    .await;
    let created = file_change(&created);

    assert_eq!(created.change_id, "change-create");
    assert_eq!(created.kind, FileChangeKind::Created);
    assert_eq!((created.additions, created.deletions), (3, 0));
    assert_eq!(created.hunks.len(), 1);
    assert_eq!(created.hunks[0].lines.len(), 3);
    assert!(created.before_content.is_none());
    assert_eq!(
        created.after_content.as_deref(),
        Some("fn main() {\n    println!(\"hello\");\n}\n")
    );

    let modified = call(
        &toolset,
        "change-modify",
        "write",
        json!({"path": path, "content": "fn main() {\n    println!(\"world\");\n}\n// done\n"}),
    )
    .await;
    let modified = file_change(&modified);

    assert_eq!(modified.kind, FileChangeKind::Modified);
    assert_eq!((modified.additions, modified.deletions), (2, 1));
    assert_eq!(
        modified.before_content.as_deref(),
        Some("fn main() {\n    println!(\"hello\");\n}\n")
    );
    assert_ne!(
        modified.before_hash.as_deref(),
        Some(modified.after_hash.as_str())
    );
}

#[tokio::test]
async fn undo_reverses_multiple_changes_to_the_same_file_in_reverse_order() {
    let root = TestDirectory::new("undo-chain");
    let context = session(root.path());
    let toolset = builtin_registry()
        .finalize(
            &ToolsetConfig::from_names(["write", "edit"]),
            context.clone(),
        )
        .expect("toolset");
    let path = root.path().join("notes.txt");

    let created = call(
        &toolset,
        "change-1",
        "write",
        json!({"path": path, "content": "alpha\n"}),
    )
    .await;
    let edited = call(
        &toolset,
        "change-2",
        "edit",
        json!({
            "filePath": path,
            "oldString": "alpha",
            "newString": "beta",
            "replaceAll": false
        }),
    )
    .await;
    let changes = vec![file_change(&created), file_change(&edited)];

    let result = undo_file_changes(&context, &changes)
        .await
        .expect("undo chained file changes");

    assert_eq!(result.undone_change_ids, ["change-2", "change-1"]);
    assert!(!path.exists(), "undoing creation removes the created file");
}

#[tokio::test]
async fn undo_refuses_to_overwrite_an_external_change() {
    let root = TestDirectory::new("undo-conflict");
    let context = session(root.path());
    let toolset = builtin_registry()
        .finalize(&ToolsetConfig::from_names(["write"]), context.clone())
        .expect("toolset");
    let path = root.path().join("README.md");
    std::fs::write(&path, "before\n").expect("write fixture");

    let changed = call(
        &toolset,
        "change-conflict",
        "write",
        json!({"path": path, "content": "after\n"}),
    )
    .await;
    let change = file_change(&changed);
    std::fs::write(&path, "external\n").expect("simulate external edit");

    let error = undo_file_changes(&context, &[change])
        .await
        .expect_err("external edit must conflict");

    assert!(matches!(error, FileChangeUndoError::Conflict { .. }));
    assert_eq!(
        std::fs::read_to_string(&path).expect("read preserved external edit"),
        "external\n"
    );
}

#[tokio::test]
async fn reapply_restores_chained_changes_in_forward_order() {
    let root = TestDirectory::new("reapply-chain");
    let context = session(root.path());
    let toolset = builtin_registry()
        .finalize(
            &ToolsetConfig::from_names(["write", "edit"]),
            context.clone(),
        )
        .expect("toolset");
    let path = root.path().join("notes.txt");

    let created = call(
        &toolset,
        "change-1",
        "write",
        json!({"path": path, "content": "alpha\n"}),
    )
    .await;
    let edited = call(
        &toolset,
        "change-2",
        "edit",
        json!({
            "filePath": path,
            "oldString": "alpha",
            "newString": "beta",
            "replaceAll": false
        }),
    )
    .await;
    let mut changes = vec![file_change(&created), file_change(&edited)];

    undo_file_changes(&context, &changes)
        .await
        .expect("undo chained file changes");
    for change in &mut changes {
        change.undone = true;
    }

    let result = reapply_file_changes(&context, &changes)
        .await
        .expect("reapply chained file changes");

    assert_eq!(result.reapplied_change_ids, ["change-1", "change-2"]);
    assert_eq!(
        std::fs::read_to_string(&path).expect("read reapplied file"),
        "beta\n"
    );
}

#[tokio::test]
async fn reapply_refuses_to_overwrite_a_change_made_after_undo() {
    let root = TestDirectory::new("reapply-conflict");
    let context = session(root.path());
    let toolset = builtin_registry()
        .finalize(&ToolsetConfig::from_names(["write"]), context.clone())
        .expect("toolset");
    let path = root.path().join("README.md");
    std::fs::write(&path, "before\n").expect("write fixture");

    let changed = call(
        &toolset,
        "change-conflict",
        "write",
        json!({"path": path, "content": "after\n"}),
    )
    .await;
    let mut change = file_change(&changed);
    undo_file_changes(&context, &[change.clone()])
        .await
        .expect("undo file change");
    change.undone = true;
    std::fs::write(&path, "external\n").expect("simulate external edit after undo");

    let error = reapply_file_changes(&context, &[change])
        .await
        .expect_err("external edit must conflict");

    assert!(
        error
            .to_string()
            .contains("changed after the recorded undo")
    );
    assert_eq!(
        std::fs::read_to_string(&path).expect("read preserved external edit"),
        "external\n"
    );
}
