//! tools.md §9 "先读后改" and §12 #31–34.

use std::path::Path;
use std::sync::{Arc, OnceLock};

use openwork_sandbox::{
    SANDBOX_EXEC, SandboxBackend, SandboxEnvironment, SandboxMode, SandboxPolicy, Seatbelt,
};
use openwork_tools::{
    FileObservations, FinalizedToolset, ToolCallContext, ToolCallId, ToolInvocation, ToolResult,
    ToolSessionContext, ToolsetConfig, builtin_registry,
};
use serde_json::{Value, json};
use tempfile::TempDir;
use tokio_util::sync::CancellationToken;

/// 真实的 Seatbelt，整个测试进程只自检一次。
fn sandbox() -> Arc<dyn SandboxBackend> {
    static SANDBOX: OnceLock<Arc<Seatbelt>> = OnceLock::new();
    SANDBOX
        .get_or_init(|| Arc::new(Seatbelt::probe(SANDBOX_EXEC)))
        .clone()
}

fn toolset(workspace: &Path, observations: FileObservations) -> FinalizedToolset {
    builtin_registry()
        .finalize(
            &ToolsetConfig::from_names(["read", "write", "edit", "bash"]),
            ToolSessionContext::local(workspace.to_path_buf(), sandbox())
                .with_file_observations(observations),
        )
        .expect("toolset")
}

async fn call(tools: &FinalizedToolset, name: &str, input: Value) -> ToolResult {
    tools
        .call(
            ToolCallContext::new(ToolCallId::new(name), CancellationToken::new(), policy()),
            ToolInvocation::new(name, input),
        )
        .await
}

/// 测试工作区在系统临时目录下，`auto` 下可写。
fn policy() -> SandboxPolicy {
    let environment = SandboxEnvironment::detect([]).expect("environment");
    let workspace = std::fs::canonicalize(std::env::temp_dir()).expect("temp dir");
    SandboxPolicy::new(SandboxMode::Auto, workspace, Arc::new(environment))
}

fn edit(old: &str, new: &str) -> Value {
    json!({ "filePath": "lib.rs", "oldString": old, "newString": new })
}

/// #31
#[tokio::test]
async fn acc_31_unread_existing_files_cannot_be_edited_or_overwritten() {
    let workspace = TempDir::new().expect("workspace");
    std::fs::write(workspace.path().join("lib.rs"), "fn a() {}\n").expect("fixture");
    let tools = toolset(workspace.path(), FileObservations::new());

    let edited = call(&tools, "edit", edit("a", "b")).await;
    assert!(edited.is_error());
    assert_eq!(edited.text_content(), "Read lib.rs before editing it.");

    let written = call(
        &tools,
        "write",
        json!({ "path": "lib.rs", "content": "replaced\n" }),
    )
    .await;
    assert!(written.is_error());
    assert_eq!(written.text_content(), "Read lib.rs before editing it.");
    assert_eq!(
        std::fs::read_to_string(workspace.path().join("lib.rs")).unwrap(),
        "fn a() {}\n"
    );
}

/// #32
#[tokio::test]
async fn acc_32_files_changed_since_the_read_must_be_read_again() {
    let workspace = TempDir::new().expect("workspace");
    std::fs::write(workspace.path().join("lib.rs"), "fn a() {}\n").expect("fixture");
    let tools = toolset(workspace.path(), FileObservations::new());
    call(&tools, "read", json!({ "path": "lib.rs" })).await;

    call(
        &tools,
        "bash",
        json!({ "command": "printf 'fn c() {}\\n' > lib.rs" }),
    )
    .await;
    let edited = call(&tools, "edit", edit("c", "d")).await;
    assert!(edited.is_error());
    assert!(
        edited
            .text_content()
            .starts_with("lib.rs changed since you last read it")
    );

    call(&tools, "read", json!({ "path": "lib.rs" })).await;
    let edited = call(&tools, "edit", edit("c", "d")).await;
    assert!(!edited.is_error(), "{}", edited.text_content());
}

/// #33
#[tokio::test]
async fn acc_33_consecutive_edits_do_not_need_a_new_read() {
    let workspace = TempDir::new().expect("workspace");
    std::fs::write(workspace.path().join("lib.rs"), "fn a() {}\nfn b() {}\n").expect("fixture");
    let tools = toolset(workspace.path(), FileObservations::new());
    call(
        &tools,
        "read",
        json!({ "path": "lib.rs", "offset": 2, "limit": 1 }),
    )
    .await;

    for (old, new) in [("fn a", "fn x"), ("fn b", "fn y")] {
        let edited = call(&tools, "edit", edit(old, new)).await;
        assert!(!edited.is_error(), "{}", edited.text_content());
    }
    let written = call(
        &tools,
        "write",
        json!({ "path": "lib.rs", "content": "fn z() {}\n" }),
    )
    .await;
    assert!(!written.is_error(), "{}", written.text_content());
}

/// #34
#[tokio::test]
async fn acc_34_creating_files_needs_no_read() {
    let workspace = TempDir::new().expect("workspace");
    let tools = toolset(workspace.path(), FileObservations::new());

    let written = call(
        &tools,
        "write",
        json!({ "path": "new.rs", "content": "fn a() {}\n" }),
    )
    .await;
    assert!(!written.is_error(), "{}", written.text_content());
    let created = call(
        &tools,
        "edit",
        json!({ "filePath": "other.rs", "oldString": "", "newString": "fn b() {}\n" }),
    )
    .await;
    assert!(!created.is_error(), "{}", created.text_content());
    let edited = call(
        &tools,
        "edit",
        json!({ "filePath": "other.rs", "oldString": "fn b", "newString": "fn c" }),
    )
    .await;
    assert!(!edited.is_error(), "{}", edited.text_content());
}

/// The table belongs to the Session, not to one toolset: Core builds a new
/// toolset per Turn and passes the same table.
#[tokio::test]
async fn observations_carry_across_toolsets_that_share_a_table() {
    let workspace = TempDir::new().expect("workspace");
    std::fs::write(workspace.path().join("lib.rs"), "fn a() {}\n").expect("fixture");
    let observations = FileObservations::new();

    let first_turn = toolset(workspace.path(), observations.clone());
    call(&first_turn, "read", json!({ "path": "lib.rs" })).await;
    let second_turn = toolset(workspace.path(), observations);
    let edited = call(&second_turn, "edit", edit("a", "b")).await;

    assert!(!edited.is_error(), "{}", edited.text_content());
}
