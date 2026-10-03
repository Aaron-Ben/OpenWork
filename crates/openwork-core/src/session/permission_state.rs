//! 会话的沙箱状态（permissions.md §13）：模式、模式的来源、有没有人能批准。
//!
//! 越界批准只作用于那一次调用，这里不保存任何授权（§6.2）。

use std::path::PathBuf;
use std::sync::Arc;

use openwork_sandbox::{PathGrant, SandboxEnvironment, SandboxMode, SandboxPolicy};
use serde::{Deserialize, Serialize};

/// Whether anyone can answer an approval prompt for this Session.
///
/// **Not a third mode.** A mode is the sandbox the user chose; this is a
/// property of the runtime environment — whether a user exists at all.
/// Sub-agent Sessions run unattended, so a card there can never be answered
/// (permissions.md §13.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionApproval {
    /// A user is in the loop: a card suspends the Tool Call and waits.
    #[default]
    Interactive,
    /// Nobody is in the loop: what would need a card is denied immediately.
    NonInteractive,
}

impl SessionApproval {
    pub fn is_interactive(self) -> bool {
        matches!(self, Self::Interactive)
    }
}

/// 子 Agent 碰到需要批准的调用时给模型的文本（permissions.md §13.3）。
///
/// 必须可操作：只说"被拒"会让模型重复同一条命令直到耗尽 `max_model_calls`。
pub fn non_interactive_denial(mode: SandboxMode, reason: &str) -> String {
    format!(
        "This sub-agent runs unattended, so nobody can approve {reason}. Keep working within the \
         {mode} sandbox (temporary directories are writable), or report what you could not do to \
         the parent agent.",
        mode = match mode {
            SandboxMode::AcceptEdits => "accept-edits",
            SandboxMode::Auto => "auto",
        }
    )
}

/// 会话模式是怎么来的（permissions.md §14.2 `sessionModeOrigin`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionModeOrigin {
    /// 会话持久化的模式：新会话的默认值，或上次切换后落库的值。
    SessionDefault,
    /// 用户在这个进程里切换过。
    UserToggle,
    /// 子 Agent 在派生时从父会话与角色上限取的快照。
    Inherited,
}

impl SessionModeOrigin {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::SessionDefault => "session_default",
            Self::UserToggle => "user_toggle",
            Self::Inherited => "inherited",
        }
    }
}

#[derive(Clone)]
pub(super) struct SessionPermissionState {
    mode: SandboxMode,
    mode_origin: SessionModeOrigin,
}

impl SessionPermissionState {
    pub(super) fn new(mode: SandboxMode, mode_origin: SessionModeOrigin) -> Self {
        Self { mode, mode_origin }
    }

    pub(super) fn mode(&self) -> SandboxMode {
        self.mode
    }

    pub(super) fn mode_origin(&self) -> SessionModeOrigin {
        self.mode_origin
    }

    pub(super) fn set_mode(&mut self, mode: SandboxMode, origin: SessionModeOrigin) {
        self.mode = mode;
        self.mode_origin = origin;
    }
}

/// 生成每次调用的 [`SandboxPolicy`] 所需的会话事实。
#[derive(Debug, Clone)]
pub struct SessionSandbox {
    /// 规范化后的工作区根：Seatbelt 按真实路径匹配。
    pub workspace_root: PathBuf,
    pub environment: Arc<SandboxEnvironment>,
}

impl SessionSandbox {
    /// 这一次调用的策略：会话模式，加上用户为这一次批准的越界路径。
    pub fn policy(&self, mode: SandboxMode, grants: Vec<PathGrant>) -> SandboxPolicy {
        SandboxPolicy::new(
            mode,
            self.workspace_root.clone(),
            Arc::clone(&self.environment),
        )
        .with_grants(grants)
    }
}
