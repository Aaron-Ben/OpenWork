//! 单元测试共用的沙箱装配。
//!
//! 真实 Seatbelt 的行为由 `openwork-sandbox` 的真机测试与本 crate 的集成测试覆盖；
//! 单元测试只关心工具自己的逻辑，所以这里的后端不做任何约束，只报告"可用"。

use std::path::Path;
use std::sync::Arc;

use openwork_sandbox::{
    SandboxBackend, SandboxEnvironment, SandboxMode, SandboxPolicy, SandboxStatus,
    SandboxUnavailable,
};

use crate::ToolSessionContext;

/// 报告可用、原样返回 argv 的后端。
#[derive(Debug)]
pub(crate) struct Unconfined(SandboxStatus);

impl Default for Unconfined {
    fn default() -> Self {
        Self(SandboxStatus::Available)
    }
}

impl SandboxBackend for Unconfined {
    fn status(&self) -> &SandboxStatus {
        &self.0
    }

    fn wrap(
        &self,
        _policy: &SandboxPolicy,
        command: &[String],
    ) -> Result<Vec<String>, SandboxUnavailable> {
        Ok(command.to_vec())
    }
}

pub(crate) fn unconfined_session(workspace: &Path) -> ToolSessionContext {
    ToolSessionContext::local(workspace.to_path_buf(), Arc::new(Unconfined::default()))
}

/// 自检失败的会话：bash 不执行，越界参数不在 schema 里。
pub(crate) fn unavailable_session(workspace: &Path) -> ToolSessionContext {
    ToolSessionContext::local(
        workspace.to_path_buf(),
        Arc::new(Unconfined(SandboxStatus::Unavailable {
            reason: "sandbox-exec is missing".to_string(),
        })),
    )
}

/// 以 `workspace` 为工作区的 `auto` 策略，路径取真实路径。
pub(crate) fn policy_for(workspace: &Path) -> SandboxPolicy {
    policy_with_mode(workspace, SandboxMode::Auto)
}

pub(crate) fn policy_with_mode(workspace: &Path, mode: SandboxMode) -> SandboxPolicy {
    let environment = SandboxEnvironment::detect([]).expect("sandbox environment");
    let workspace = std::fs::canonicalize(workspace).unwrap_or_else(|_| workspace.to_path_buf());
    SandboxPolicy::new(mode, workspace, Arc::new(environment))
}

/// 以系统临时目录为工作区的调用上下文；单元测试的目录都建在临时目录下，写入在
/// `auto` 下被允许。
pub(crate) fn call_context(
    id: &str,
    cancel: tokio_util::sync::CancellationToken,
) -> crate::ToolCallContext {
    crate::ToolCallContext::new(
        crate::ToolCallId::new(id),
        cancel,
        policy_for(&std::env::temp_dir()),
    )
}

/// 只有工作区可写（没有可写的临时目录）、以 `home` 为主目录的调用上下文，用来检查围栏：
/// 单元测试的目录都在系统临时目录下，用真实环境时那里处处可写。
pub(crate) fn fenced_call(id: &str, workspace: &Path, home: &Path) -> crate::ToolCallContext {
    let canonical =
        |path: &Path| std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let environment = SandboxEnvironment::new(canonical(home), Vec::new(), Vec::new());
    crate::ToolCallContext::new(
        crate::ToolCallId::new(id),
        tokio_util::sync::CancellationToken::new(),
        SandboxPolicy::new(
            SandboxMode::Auto,
            canonical(workspace),
            Arc::new(environment),
        ),
    )
}
