//! Core 为一个 Session 装配工具与沙箱（permissions.md §4、§6，tools.md §4）。

use std::path::{Path, PathBuf};
use std::sync::Arc;

use openwork_agent::{Agent, AgentBuilder, AgentDefinition, explorer_definition};
use openwork_sandbox::{
    Access, GrantScope, PathGrant, SANDBOX_EXEC, SandboxBackend, SandboxEnvironment, SandboxMode,
    SandboxPolicy, SandboxStatus, Seatbelt,
};
use openwork_tools::{
    FileChangeArtifact, FileObservations, FinalizedToolset, SpillDirectory, ToolSessionContext,
    builtin_registry,
};

use crate::OpenWorkCoreError;
use crate::session::{
    COMPACTION_TRANSCRIPT_TOOL_NAME, ConversationTranscriptTool, SessionId, SessionSandbox,
    SessionStorage,
};
use crate::skills::SkillRoots;

/// 进程级的沙箱：启动自检的结论与主机事实，整个进程只算一次。
#[derive(Clone)]
pub(crate) struct SandboxRuntime {
    backend: Arc<dyn SandboxBackend>,
    environment: Arc<SandboxEnvironment>,
}

impl SandboxRuntime {
    /// 自检要运行两次 `sandbox-exec`，环境探测要规范化路径：都放到阻塞线程上。
    pub(crate) async fn start(skill_roots: &SkillRoots) -> Result<Self, OpenWorkCoreError> {
        let skill_roots = skill_roots.agents.iter().cloned().collect::<Vec<_>>();
        tokio::task::spawn_blocking(move || {
            let environment = SandboxEnvironment::detect(skill_roots)
                .map_err(OpenWorkCoreError::SandboxEnvironment)?;
            let backend: Arc<dyn SandboxBackend> = Arc::new(Seatbelt::probe(SANDBOX_EXEC));
            if let SandboxStatus::Unavailable { reason } = backend.status() {
                tracing::warn!(%reason, "sandbox self-check failed; bash is disabled");
            }
            Ok(Self {
                backend,
                environment: Arc::new(environment),
            })
        })
        .await
        .map_err(OpenWorkCoreError::SandboxStartupTask)?
    }

    /// 一个 Session 的策略工厂。工作区取真实路径：Seatbelt 按真实路径匹配。
    pub(crate) async fn session(&self, working_directory: &Path) -> SessionSandbox {
        let workspace_root = tokio::fs::canonicalize(working_directory)
            .await
            .unwrap_or_else(|_| working_directory.to_path_buf());
        SessionSandbox {
            workspace_root,
            environment: Arc::clone(&self.environment),
        }
    }

    /// 撤销与重新应用文件改动的策略。这是用户在界面上的操作：除了会话模式，还为这批改动
    /// 涉及的每个文件给出精确写授权，使经越界批准改过的敏感文件（如 `.env`）也能撤销；
    /// 硬保护路径不受授权影响，照样不可写。
    pub(crate) async fn file_change_policy(
        &self,
        working_directory: &Path,
        mode: SandboxMode,
        changes: &[FileChangeArtifact],
    ) -> SandboxPolicy {
        let session = self.session(working_directory).await;
        let mut grants = Vec::with_capacity(changes.len());
        for change in changes {
            let path = session.workspace_root.join(&change.path);
            let path = tokio::fs::canonicalize(&path).await.unwrap_or(path);
            grants.push(PathGrant {
                path,
                access: Access::Write,
                scope: GrantScope::Exact,
            });
        }
        session.policy(mode, grants)
    }

    /// 撤销与重新应用改动用的工具上下文：只用到文件系统，不需要落盘与先读后改。
    pub(crate) fn file_change_context(&self, working_directory: PathBuf) -> ToolSessionContext {
        ToolSessionContext::local(working_directory, Arc::clone(&self.backend))
    }

    fn tool_session_context(
        &self,
        working_directory: &Path,
        state: SessionToolState,
    ) -> ToolSessionContext {
        let context =
            ToolSessionContext::local(working_directory.to_path_buf(), Arc::clone(&self.backend))
                .with_file_observations(state.observations);
        match state.spill {
            Some(spill) => context.with_spill_directory(spill),
            None => context,
        }
    }

    pub(crate) fn build_default_agent_and_tools(
        &self,
        session_id: &SessionId,
        working_directory: &Path,
        state: SessionToolState,
        storage: Arc<dyn SessionStorage>,
    ) -> Result<(Agent, FinalizedToolset), OpenWorkCoreError> {
        let mut definition = AgentDefinition::default();
        definition
            .tool_names
            .push(COMPACTION_TRANSCRIPT_TOOL_NAME.to_string());
        let agent = AgentBuilder::new(definition)
            .build()
            .map_err(|error| OpenWorkCoreError::RuntimeComponent(error.to_string()))?;
        let tools = builtin_registry()
            .register(ConversationTranscriptTool::new(session_id.clone(), storage))
            .finalize(
                agent.toolset_config(),
                self.tool_session_context(working_directory, state),
            )
            .map_err(|error| OpenWorkCoreError::RuntimeComponent(error.to_string()))?;
        Ok((agent, tools))
    }

    pub(crate) fn build_explorer_agent_and_tools(
        &self,
        working_directory: &Path,
        state: SessionToolState,
    ) -> Result<(Agent, FinalizedToolset), OpenWorkCoreError> {
        let agent = AgentBuilder::new(explorer_definition())
            .build()
            .map_err(|error| OpenWorkCoreError::RuntimeComponent(error.to_string()))?;
        let tools = builtin_registry()
            .finalize(
                agent.toolset_config(),
                self.tool_session_context(working_directory, state),
            )
            .map_err(|error| OpenWorkCoreError::RuntimeComponent(error.to_string()))?;
        Ok((agent, tools))
    }
}

/// 子 Agent 的生效模式：父会话模式与角色上限中较窄者（permissions.md §13.3）。
pub(crate) fn sub_agent_mode(parent: SandboxMode) -> SandboxMode {
    parent.min(explorer_definition().sandbox_ceiling)
}

/// Per-session tool state that Core keeps across Turns.
#[derive(Debug, Clone, Default)]
pub(crate) struct SessionToolState {
    pub(crate) spill: Option<SpillDirectory>,
    pub(crate) observations: FileObservations,
}

#[cfg(test)]
impl SandboxRuntime {
    /// 真实的 Seatbelt 自检，加上测试给定的主机事实（主目录决定 `~/.openwork` 在哪）。
    fn for_test(environment: SandboxEnvironment) -> Self {
        Self {
            backend: Arc::new(Seatbelt::probe(SANDBOX_EXEC)),
            environment: Arc::new(environment),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use openwork_tools::{ToolCallContext, ToolCallId, ToolInvocation, ToolRisk, ToolsetConfig};
    use serde_json::json;
    use tokio_util::sync::CancellationToken;

    use super::*;
    use crate::session::NoopSessionStorage;
    use crate::spill::SpillRoot;

    /// 测试根：`home/` 充当主目录，`home/project` 是工作区。
    struct Fixture {
        root: PathBuf,
        home: PathBuf,
        workspace: PathBuf,
    }

    impl Fixture {
        fn new(name: &str) -> Self {
            let root = std::env::temp_dir().join(format!(
                "openwork-core-{name}-{}",
                uuid::Uuid::new_v4().simple()
            ));
            std::fs::create_dir_all(root.join("home/project")).expect("workspace");
            let root = std::fs::canonicalize(root).expect("canonical root");
            Self {
                home: root.join("home"),
                workspace: root.join("home/project"),
                root,
            }
        }

        fn runtime(&self, skill_roots: Vec<PathBuf>) -> SandboxRuntime {
            let skill_roots = skill_roots
                .into_iter()
                .map(|root| std::fs::canonicalize(&root).unwrap_or(root))
                .collect();
            SandboxRuntime::for_test(SandboxEnvironment::new(
                self.home.clone(),
                vec![std::fs::canonicalize("/tmp").expect("tmp")],
                skill_roots,
            ))
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            if let Err(error) = std::fs::remove_dir_all(&self.root) {
                eprintln!("cleanup {}: {error}", self.root.display());
            }
        }
    }

    async fn call(
        tools: &FinalizedToolset,
        policy: &SandboxPolicy,
        id: &str,
        invocation: ToolInvocation,
    ) -> openwork_tools::ToolResult {
        tools
            .call(
                ToolCallContext::new(
                    ToolCallId::new(id),
                    CancellationToken::new(),
                    policy.clone(),
                ),
                invocation,
            )
            .await
    }

    #[tokio::test]
    async fn the_explorer_exposes_only_the_read_only_role_surface() {
        let fixture = Fixture::new("explorer-toolset");
        let (agent, tools) = fixture
            .runtime(Vec::new())
            .build_explorer_agent_and_tools(&fixture.workspace, SessionToolState::default())
            .expect("explorer toolset");

        assert_eq!(agent.definition().name, "explorer");
        assert_eq!(
            tools
                .definitions()
                .iter()
                .map(|definition| definition.name.as_str())
                .collect::<Vec<_>>(),
            ["read", "grep", "glob", "list", "bash"]
        );
    }

    #[tokio::test]
    async fn the_default_runtime_exposes_checkpoint_bounded_history_readback() {
        let fixture = Fixture::new("history-tool");
        let (_, tools) = fixture
            .runtime(Vec::new())
            .build_default_agent_and_tools(
                &SessionId::new("session-history-tool"),
                &fixture.workspace,
                SessionToolState::default(),
                Arc::new(NoopSessionStorage),
            )
            .expect("default toolset");
        let definition = tools
            .resolve(COMPACTION_TRANSCRIPT_TOOL_NAME)
            .expect("conversation history tool");
        assert_eq!(definition.risk_hint, ToolRisk::ReadOnly);
    }

    /// permissions.md §13.3：子 Agent 取父会话模式与 explorer 上限中较窄者。
    #[test]
    fn a_sub_agent_never_gets_a_wider_mode_than_its_parent_or_role() {
        assert_eq!(sub_agent_mode(SandboxMode::Auto), SandboxMode::AcceptEdits);
        assert_eq!(
            sub_agent_mode(SandboxMode::AcceptEdits),
            SandboxMode::AcceptEdits
        );
    }

    /// tools.md §10 #26：落盘文件不经审批可读，任何模式下都不可写（`~/.openwork` 硬保护）。
    #[cfg(target_os = "macos")]
    #[tokio::test]
    async fn acc_26_spilled_output_is_readable_and_never_writable() {
        let fixture = Fixture::new("spill");
        let runtime = fixture.runtime(Vec::new());
        let spill_root = SpillRoot::new(fixture.home.join(".openwork/spill"));
        let session_id = SessionId::new("session-spill");
        let (_, tools) = runtime
            .build_default_agent_and_tools(
                &session_id,
                &fixture.workspace,
                SessionToolState {
                    spill: Some(spill_root.session(&session_id)),
                    ..SessionToolState::default()
                },
                Arc::new(NoopSessionStorage),
            )
            .expect("default toolset");
        let policy = runtime
            .session(&fixture.workspace)
            .await
            .policy(SandboxMode::Auto, Vec::new());

        let output = call(
            &tools,
            &policy,
            "call-seq",
            ToolInvocation::new("bash", json!({ "command": "seq 1 20000" })),
        )
        .await
        .text_content();
        let spilled = spill_root.session(&session_id).path().join("call-seq.txt");
        assert!(output.contains(&format!("Full output saved at {}", spilled.display())));

        let read_back = call(
            &tools,
            &policy,
            "call-read",
            ToolInvocation::new(
                "read",
                json!({ "path": &spilled, "offset": 19_999, "limit": 2 }),
            ),
        )
        .await
        .text_content();
        assert_eq!(read_back, "19999\t19999\n20000\t20000");

        let write =
            ToolInvocation::new("write", json!({ "path": &spilled, "content": "tampered" }));
        let prepared = tools.prepare(&write, &policy).await.expect("valid write");
        assert!(
            prepared.protected_target.is_some(),
            "the spill file is hard-protected"
        );
        let denied = call(&tools, &policy, "call-write", write).await;
        assert!(denied.is_error());
        assert!(
            std::fs::read_to_string(&spilled)
                .expect("spill file")
                .starts_with("1\n2\n")
        );
    }

    /// skills.md §5.2：配置的 skill 根（含经符号链接配置的真实路径）硬保护；工作区里同名的
    /// `.agents/skills` 只是普通文件。
    #[cfg(unix)]
    #[tokio::test]
    async fn only_the_configured_skill_root_is_protected_even_through_a_symlink() {
        use std::os::unix::fs::symlink;

        let fixture = Fixture::new("skill-roots");
        let actual_root = fixture.home.join("actual-skills");
        let configured_root = fixture.home.join(".agents/skills");
        let skill = actual_root.join("review/SKILL.md");
        std::fs::create_dir_all(skill.parent().expect("skill directory")).expect("skill dir");
        std::fs::create_dir_all(configured_root.parent().expect("agents")).expect("agents");
        std::fs::write(&skill, "Body\n").expect("skill");
        symlink(&actual_root, &configured_root).expect("skill root symlink");

        let runtime = fixture.runtime(vec![configured_root.clone()]);
        let policy = runtime
            .session(&fixture.workspace)
            .await
            .policy(SandboxMode::Auto, Vec::new());
        let tools = builtin_registry()
            .finalize(
                &ToolsetConfig::from_names(["read", "write"]),
                runtime.file_change_context(fixture.workspace.clone()),
            )
            .expect("toolset");

        for path in [skill.clone(), configured_root.join("review/SKILL.md")] {
            let read = call(
                &tools,
                &policy,
                "call-read",
                ToolInvocation::new("read", json!({ "path": &path })),
            )
            .await;
            assert_eq!(read.text_content(), "1\tBody");
            let write = ToolInvocation::new("write", json!({ "path": &path, "content": "x" }));
            let prepared = tools.prepare(&write, &policy).await.expect("valid write");
            assert!(prepared.protected_target.is_some(), "{}", path.display());
        }

        let project_skill = fixture.workspace.join(".agents/skills/local/SKILL.md");
        let write = ToolInvocation::new(
            "write",
            json!({ "path": &project_skill, "content": "ordinary workspace file" }),
        );
        let prepared = tools.prepare(&write, &policy).await.expect("valid write");
        assert!(prepared.protected_target.is_none());
    }
}
