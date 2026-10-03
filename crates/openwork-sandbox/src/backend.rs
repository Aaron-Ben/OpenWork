//! 工具眼中的沙箱：一个自检结论，加上一个包装 argv 的方法。

use std::path::{Path, PathBuf};

use crate::confinement::EngineConfinement;
use crate::policy::{Actor, SandboxPolicy};
use crate::probe::{SandboxStatus, probe};
use crate::seatbelt::SeatbeltProfile;

/// 把命令包装成由内核按策略约束的形式。只生成 argv，进程由 `ProcessBackend` 启动。
pub trait SandboxBackend: Send + Sync + std::fmt::Debug {
    /// 启动自检的结论，在进程生命周期内不变。
    fn status(&self) -> &SandboxStatus;

    /// 在 `policy` 下运行 `command` 的 argv；不能运行时返回原因。
    fn wrap(
        &self,
        policy: &SandboxPolicy,
        command: &[String],
    ) -> Result<Vec<String>, SandboxUnavailable>;
}

/// bash 不能运行：自检失败（permissions.md §6）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SandboxUnavailable {
    pub reason: String,
}

impl std::fmt::Display for SandboxUnavailable {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "sandbox unavailable: {}", self.reason)
    }
}

impl std::error::Error for SandboxUnavailable {}

/// 经 `sandbox-exec` 使用的 macOS Seatbelt。
#[derive(Debug, Clone)]
pub struct Seatbelt {
    sandbox_exec: PathBuf,
    status: SandboxStatus,
}

impl Seatbelt {
    /// 只做一次自检，结论在进程生命周期内有效。
    pub fn probe(sandbox_exec: impl Into<PathBuf>) -> Self {
        let sandbox_exec = sandbox_exec.into();
        let status = probe(&sandbox_exec);
        Self {
            sandbox_exec,
            status,
        }
    }

    pub fn sandbox_exec(&self) -> &Path {
        &self.sandbox_exec
    }

    /// 在 `confinement` 内运行 `command` 的 argv（collaboration.md §3.1）；自检失败时返回原因，
    /// 调用方不得退回无沙箱运行。
    pub fn confine(
        &self,
        confinement: &EngineConfinement,
        command: &[String],
    ) -> Result<Vec<String>, SandboxUnavailable> {
        match &self.status {
            SandboxStatus::Available => {
                Ok(SeatbeltProfile::confined(confinement).wrap(&self.sandbox_exec, command))
            }
            SandboxStatus::Unavailable { reason } => Err(SandboxUnavailable {
                reason: reason.clone(),
            }),
        }
    }
}

impl SandboxBackend for Seatbelt {
    fn status(&self) -> &SandboxStatus {
        &self.status
    }

    fn wrap(
        &self,
        policy: &SandboxPolicy,
        command: &[String],
    ) -> Result<Vec<String>, SandboxUnavailable> {
        match &self.status {
            SandboxStatus::Available => {
                Ok(SeatbeltProfile::new(policy, Actor::Bash).wrap(&self.sandbox_exec, command))
            }
            SandboxStatus::Unavailable { reason } => Err(SandboxUnavailable {
                reason: reason.clone(),
            }),
        }
    }
}
