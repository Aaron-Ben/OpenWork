//! skill 根（skills.md §5.2、permissions.md §2.3）：可读，任何模式、任何越界下都不可写。

use std::fs;
use std::path::Path;
use std::sync::Arc;

use openwork_sandbox::{SANDBOX_EXEC, SandboxEnvironment, SandboxMode, SandboxPolicy, Seatbelt};
use openwork_tools::{
    FinalizedToolset, ToolCallContext, ToolCallId, ToolInvocation, ToolResult, ToolResultStatus,
    ToolSessionContext, ToolsetConfig, builtin_registry,
};
use serde_json::json;
use tempfile::TempDir;
use tokio_util::sync::CancellationToken;

#[tokio::test]
async fn read_can_open_an_agents_skill_outside_the_workspace() {
    let workspace = TempDir::new().expect("workspace");
    let agents_skills = TempDir::new().expect("agents skill root");
    let skill_directory = agents_skills.path().join("review");
    fs::create_dir_all(&skill_directory).expect("skill directory");
    let skill_path = skill_directory.join("SKILL.md");
    fs::write(&skill_path, "instructions from the user skill\n").expect("skill");
    let tools = skill_toolset(workspace.path(), ["read"]);
    let policy = policy(workspace.path(), agents_skills.path(), SandboxMode::Auto);

    let result = call(&tools, "read", json!({ "path": skill_path }), policy).await;

    assert_eq!(result.text_content(), "1\tinstructions from the user skill");
}

#[tokio::test]
async fn all_read_only_file_tools_can_use_the_agents_root() {
    let workspace = TempDir::new().expect("workspace");
    let agents_skills = TempDir::new().expect("agents skill root");
    let skill_directory = agents_skills.path().join("review");
    fs::create_dir_all(skill_directory.join("references")).expect("skill directory");
    fs::write(
        skill_directory.join("SKILL.md"),
        "unique skill instructions\n",
    )
    .expect("skill");
    fs::write(
        skill_directory.join("references/api.md"),
        "reference details\n",
    )
    .expect("reference");
    let tools = skill_toolset(workspace.path(), ["read", "grep", "glob", "list"]);
    let policy = policy(workspace.path(), agents_skills.path(), SandboxMode::Auto);

    let cases = [
        (
            "read",
            json!({ "path": skill_directory.join("SKILL.md") }),
            "unique skill instructions",
        ),
        (
            "read",
            json!({ "path": skill_directory.join("references/api.md") }),
            "reference details",
        ),
        (
            "grep",
            json!({ "pattern": "unique skill", "path": skill_directory }),
            "SKILL.md\n1:unique skill instructions",
        ),
        (
            "glob",
            json!({ "pattern": "**/*.md", "path": skill_directory }),
            "SKILL.md",
        ),
        ("list", json!({ "path": skill_directory }), "SKILL.md"),
    ];
    for (tool_name, input, expected) in cases {
        let result = call(&tools, tool_name, input, policy.clone()).await;
        assert!(
            result.text_content().contains(expected),
            "{tool_name} must read {}: {:?}",
            agents_skills.path().display(),
            result.text_content()
        );
    }
}

/// 写目标是硬保护路径时，Core 在执行前就能知道（规则拒绝，不出卡片），执行时围栏也拒绝。
#[tokio::test]
async fn write_and_edit_are_denied_for_the_agents_skill_root_in_every_mode() {
    let workspace = TempDir::new().expect("workspace");
    let agents_skills = TempDir::new().expect("agents skill root");
    let skill_directory = agents_skills.path().join("commit");
    fs::create_dir_all(&skill_directory).expect("skill directory");
    let path = skill_directory.join("SKILL.md");
    fs::write(&path, "old\n").expect("skill");
    let tools = skill_toolset(workspace.path(), ["read", "write", "edit"]);

    for mode in [SandboxMode::Auto, SandboxMode::AcceptEdits] {
        let policy = policy(workspace.path(), agents_skills.path(), mode);
        call(&tools, "read", json!({ "path": path }), policy.clone()).await;
        for invocation in [
            ToolInvocation::new("write", json!({ "path": path, "content": "changed" })),
            ToolInvocation::new(
                "edit",
                json!({ "filePath": path, "oldString": "old", "newString": "changed" }),
            ),
        ] {
            let prepared = tools.prepare(&invocation, &policy).await.expect("prepared");
            let protected = prepared.protected_target.expect("hard protected");
            assert!(
                protected
                    .ends_with("is protected and cannot be written in any mode; do not retry]")
            );

            let result = tools
                .call(
                    context(&invocation.name, policy.clone()),
                    invocation.clone(),
                )
                .await;
            assert_eq!(result.status, ToolResultStatus::Denied, "{mode:?}");
        }
    }
    assert_eq!(fs::read_to_string(path).expect("unchanged skill"), "old\n");
}

#[cfg(unix)]
#[tokio::test]
async fn an_alias_cannot_bypass_canonical_skill_root_write_protection() {
    use std::os::unix::fs::symlink;

    let workspace = TempDir::new().expect("workspace");
    let agents_skills = TempDir::new().expect("agents skill root");
    let alias_parent = TempDir::new().expect("alias parent");
    let skill_directory = agents_skills.path().join("commit");
    fs::create_dir_all(&skill_directory).expect("skill directory");
    let skill_path = skill_directory.join("SKILL.md");
    fs::write(&skill_path, "original\n").expect("skill");
    let alias = alias_parent.path().join("skill-alias");
    symlink(agents_skills.path(), &alias).expect("skill root alias");
    let tools = skill_toolset(workspace.path(), ["write"]);
    let policy = policy(workspace.path(), agents_skills.path(), SandboxMode::Auto);
    let invocation = ToolInvocation::new(
        "write",
        json!({ "path": alias.join("commit/SKILL.md"), "content": "changed\n" }),
    );

    let prepared = tools.prepare(&invocation, &policy).await.expect("prepared");
    assert!(prepared.protected_target.is_some());
    let result = tools
        .call(context("write-skill-alias", policy), invocation)
        .await;

    assert_eq!(result.status, ToolResultStatus::Denied);
    assert_eq!(
        fs::read_to_string(skill_path).expect("unchanged skill"),
        "original\n"
    );
}

fn skill_toolset<const N: usize>(workspace: &Path, tool_names: [&str; N]) -> FinalizedToolset {
    builtin_registry()
        .finalize(
            &ToolsetConfig::from_names(tool_names),
            ToolSessionContext::local(
                workspace.to_path_buf(),
                Arc::new(Seatbelt::probe(SANDBOX_EXEC)),
            ),
        )
        .expect("skill toolset")
}

/// 以 `skill_root` 为硬保护 skill 根的策略；主目录与临时目录取真实环境。
fn policy(workspace: &Path, skill_root: &Path, mode: SandboxMode) -> SandboxPolicy {
    let detected = SandboxEnvironment::detect([]).expect("environment");
    let canonical = |path: &Path| fs::canonicalize(path).expect("canonical");
    let environment = SandboxEnvironment::new(
        detected.home().to_path_buf(),
        detected.temp_roots().to_vec(),
        vec![canonical(skill_root)],
    );
    SandboxPolicy::new(mode, canonical(workspace), Arc::new(environment))
}

fn context(id: &str, policy: SandboxPolicy) -> ToolCallContext {
    ToolCallContext::new(ToolCallId::new(id), CancellationToken::new(), policy)
}

async fn call(
    tools: &FinalizedToolset,
    name: &str,
    input: serde_json::Value,
    policy: SandboxPolicy,
) -> ToolResult {
    tools
        .call(
            context(&format!("{name}-skill"), policy),
            ToolInvocation::new(name, input),
        )
        .await
}
