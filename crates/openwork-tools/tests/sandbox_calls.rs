//! 工具在真实 Seatbelt 下的行为（tools.md §10 #10、#10b–10e、#26；permissions.md §5–§7、§11）。
//!
//! 工作区建在 `CARGO_TARGET_TMPDIR` 下，不在系统临时目录里：临时目录在两个模式下都可写，
//! 放在那里就看不出模式之间的差别。

#![cfg(target_os = "macos")]

use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

use openwork_sandbox::{
    Access, GrantScope, PathGrant, SANDBOX_EXEC, SandboxBackend, SandboxEnvironment, SandboxMode,
    SandboxPolicy, SandboxStatus, SandboxUnavailable, Seatbelt,
};
use openwork_tools::{
    FinalizedToolset, ToolCallContext, ToolCallId, ToolErrorCode, ToolInvocation, ToolResult,
    ToolSessionContext, ToolsetConfig, builtin_registry,
};
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;

fn seatbelt() -> Arc<dyn SandboxBackend> {
    static SANDBOX: OnceLock<Arc<Seatbelt>> = OnceLock::new();
    SANDBOX
        .get_or_init(|| Arc::new(Seatbelt::probe(SANDBOX_EXEC)))
        .clone()
}

struct Workspace {
    root: PathBuf,
}

impl Workspace {
    fn new(label: &str) -> Self {
        let base = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
        std::fs::create_dir_all(&base).expect("target tmp");
        let root = std::fs::canonicalize(base)
            .expect("canonical")
            .join(format!("sandbox-calls-{label}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("workspace");
        Self { root }
    }

    fn policy(&self, mode: SandboxMode) -> SandboxPolicy {
        let environment = SandboxEnvironment::detect([]).expect("environment");
        SandboxPolicy::new(mode, self.root.clone(), Arc::new(environment))
    }

    fn tools(&self, sandbox: Arc<dyn SandboxBackend>) -> FinalizedToolset {
        builtin_registry()
            .finalize(
                &ToolsetConfig::from_names(["read", "write", "bash"]),
                ToolSessionContext::local(self.root.clone(), sandbox),
            )
            .expect("toolset")
    }
}

impl Drop for Workspace {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root); // 清理失败只留下 target 下的临时目录
    }
}

async fn call(
    tools: &FinalizedToolset,
    policy: SandboxPolicy,
    name: &str,
    input: Value,
) -> ToolResult {
    tools
        .call(
            ToolCallContext::new(ToolCallId::new(name), CancellationToken::new(), policy),
            ToolInvocation::new(name, input),
        )
        .await
}

async fn bash(tools: &FinalizedToolset, policy: SandboxPolicy, command: &str) -> ToolResult {
    call(tools, policy, "bash", json!({ "command": command })).await
}

fn home_probe(label: &str) -> PathBuf {
    let home = PathBuf::from(std::env::var_os("HOME").expect("HOME"));
    home.join(format!(
        "openwork-sandbox-calls-{label}-{}",
        std::process::id()
    ))
}

fn quote(path: &Path) -> String {
    format!("'{}'", path.to_string_lossy().replace('\'', r"'\''"))
}

/// #10c：bash 的进程树在本次调用策略生成的 Seatbelt profile 下执行。
#[tokio::test]
async fn acc_10c_bash_runs_under_the_policy_of_this_call() {
    let workspace = Workspace::new("modes");
    let tools = workspace.tools(seatbelt());

    let auto = bash(
        &tools,
        workspace.policy(SandboxMode::Auto),
        "touch made-in-auto",
    )
    .await;
    assert!(!auto.sandbox_denied, "{}", auto.text_content());
    assert!(workspace.root.join("made-in-auto").exists());

    let accept = bash(
        &tools,
        workspace.policy(SandboxMode::AcceptEdits),
        "touch made-in-accept",
    )
    .await;
    assert!(accept.sandbox_denied, "{}", accept.text_content());
    assert!(!workspace.root.join("made-in-accept").exists());

    let written = call(
        &tools,
        workspace.policy(SandboxMode::AcceptEdits),
        "write",
        json!({ "path": "by-write-tool.txt", "content": "x" }),
    )
    .await;
    assert!(!written.is_error(), "{}", written.text_content());
}

/// #10e 与 permissions.md §11：被内核拒绝的结果标记 `sandbox_denied`，并在末尾附拒绝标记与越界提示。
#[tokio::test]
async fn acc_10e_kernel_denials_are_marked_with_the_escalation_hint() {
    let workspace = Workspace::new("denied");
    let tools = workspace.tools(seatbelt());
    let target = home_probe("denied");

    let result = bash(
        &tools,
        workspace.policy(SandboxMode::Auto),
        &format!("printf x > {}", quote(&target)),
    )
    .await;
    let _ = std::fs::remove_file(&target); // 回归时可能写成功，清理后再断言

    assert!(result.sandbox_denied);
    let text = result.text_content();
    assert!(
        text.ends_with(
            "[sandbox: file access denied under auto mode]\n[sandbox: to proceed, retry this exact command once with sandboxPermissions listing only the paths it needs, and a one-sentence justification; the user will be asked. If the paths cannot be listed, ask the user to run the command instead]"
        ),
        "{text}"
    );
}

/// #10：越界只改变这一次调用的策略；下一次调用回到会话模式，硬保护路径照样不可写。
#[tokio::test]
async fn acc_10_an_escalation_widens_only_this_call_and_never_hard_protected_paths() {
    let workspace = Workspace::new("grant");
    let tools = workspace.tools(seatbelt());
    let target = home_probe("grant");
    let command = format!("printf x > {}", quote(&target));
    let granted = workspace
        .policy(SandboxMode::Auto)
        .with_grants(vec![PathGrant {
            path: target.clone(),
            access: Access::Write,
            scope: GrantScope::Exact,
        }]);

    let with_grant = bash(&tools, granted, &command).await;
    assert!(!with_grant.sandbox_denied, "{}", with_grant.text_content());
    assert!(target.exists());
    std::fs::remove_file(&target).expect("remove probe");

    let without = bash(&tools, workspace.policy(SandboxMode::Auto), &command).await;
    let _ = std::fs::remove_file(&target);
    assert!(without.sandbox_denied, "{}", without.text_content());

    let openwork = PathBuf::from(std::env::var_os("HOME").expect("HOME")).join(".openwork");
    let protected = openwork.join(format!("openwork-sandbox-calls-{}", std::process::id()));
    let over_protected = workspace
        .policy(SandboxMode::Auto)
        .with_grants(vec![PathGrant {
            path: openwork,
            access: Access::Write,
            scope: GrantScope::Subtree,
        }]);
    let result = bash(
        &tools,
        over_protected,
        &format!("printf x > {}", quote(&protected)),
    )
    .await;
    let _ = std::fs::remove_file(&protected);
    assert!(result.sandbox_denied, "{}", result.text_content());
}

/// #26：落盘目录在 `~/.openwork` 下，沙箱内的 bash 写不进去。
#[tokio::test]
async fn acc_26_bash_cannot_write_the_spill_directory() {
    let workspace = Workspace::new("spill");
    let tools = workspace.tools(seatbelt());
    let home = PathBuf::from(std::env::var_os("HOME").expect("HOME"));
    let target = home
        .join(".openwork/spill")
        .join(format!("openwork-sandbox-calls-{}", std::process::id()));

    let result = bash(
        &tools,
        workspace.policy(SandboxMode::Auto),
        &format!("mkdir -p {}", quote(&target)),
    )
    .await;
    let _ = std::fs::remove_dir(&target);

    assert!(result.sandbox_denied, "{}", result.text_content());
}

/// #10d：自检失败时 bash 不执行，返回 `sandbox_unavailable`，没有不经沙箱的退路。
#[tokio::test]
async fn acc_10d_bash_does_not_run_when_the_sandbox_is_unavailable() {
    let workspace = Workspace::new("unavailable");
    let tools = workspace.tools(Arc::new(Seatbelt::probe("/nonexistent/sandbox-exec")));

    let result = bash(&tools, workspace.policy(SandboxMode::Auto), "touch ran").await;

    assert_eq!(
        result.error.as_ref().map(|error| error.code),
        Some(ToolErrorCode::SandboxUnavailable)
    );
    assert!(!result.sandbox_denied);
    assert!(!workspace.root.join("ran").exists());
    assert!(result.text_content().starts_with("[sandbox: unavailable"));
}

/// 启动前就失败的 `sandbox-exec`：命令没有执行。
#[derive(Debug)]
struct BrokenRunner(SandboxStatus);

impl SandboxBackend for BrokenRunner {
    fn status(&self) -> &SandboxStatus {
        &self.0
    }

    fn wrap(
        &self,
        _policy: &SandboxPolicy,
        _command: &[String],
    ) -> Result<Vec<String>, SandboxUnavailable> {
        Ok(vec![
            "/bin/sh".to_string(),
            "-c".to_string(),
            "echo 'sandbox-exec: invalid profile' >&2; exit 65".to_string(),
        ])
    }
}

/// #10e：`sandbox-exec` 启动失败归为沙箱不可用，不标 `sandbox_denied`。
#[tokio::test]
async fn acc_10e_a_runner_failure_is_unavailable_not_denied() {
    let workspace = Workspace::new("runner");
    let tools = workspace.tools(Arc::new(BrokenRunner(SandboxStatus::Available)));

    let result = bash(&tools, workspace.policy(SandboxMode::Auto), "true").await;

    assert_eq!(
        result.error.as_ref().map(|error| error.code),
        Some(ToolErrorCode::SandboxUnavailable)
    );
    assert!(!result.sandbox_denied);
}

/// #10b：凭据目录对文件工具与 bash 同样不可读。用一个假的主目录，免得碰真实凭据。
#[tokio::test]
async fn acc_10b_credential_directories_are_unreadable_for_file_tools_and_bash() {
    let workspace = Workspace::new("credentials");
    let home = workspace.root.join("home");
    std::fs::create_dir_all(home.join(".ssh")).expect("credential directory");
    std::fs::write(home.join(".ssh/id_rsa"), "secret").expect("key");
    let detected = SandboxEnvironment::detect([]).expect("environment");
    let environment =
        SandboxEnvironment::new(home.clone(), detected.temp_roots().to_vec(), Vec::new());
    let policy = SandboxPolicy::new(
        SandboxMode::Auto,
        workspace.root.clone(),
        Arc::new(environment),
    );
    let tools = workspace.tools(seatbelt());
    let key = home.join(".ssh/id_rsa");

    let read = call(&tools, policy.clone(), "read", json!({ "path": key })).await;
    assert!(read.sandbox_denied, "{}", read.text_content());
    let cat = bash(&tools, policy, &format!("cat {}", quote(&key))).await;
    assert!(cat.sandbox_denied, "{}", cat.text_content());
    assert!(!cat.text_content().contains("secret"));
}

/// permissions.md §5 工具链缓存：bash 的环境里 `GOCACHE` 指向 OpenWork 私有的临时目录。
#[tokio::test]
async fn bash_sees_the_private_go_cache() {
    let workspace = Workspace::new("gocache");
    let tools = workspace.tools(seatbelt());
    let policy = workspace.policy(SandboxMode::Auto);
    let expected = policy.environment().bash_environment();

    let result = bash(&tools, policy, "printf %s \"$GOCACHE\"").await;

    let (_, cache) = expected.first().expect("GOCACHE");
    assert!(
        result
            .text_content()
            .starts_with(&cache.to_string_lossy().into_owned()),
        "{}",
        result.text_content()
    );
}
