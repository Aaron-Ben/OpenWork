//! The sandbox as the tools see it: a probed status and a way to wrap argv.

use std::path::{Path, PathBuf};

use crate::policy::{Actor, SandboxPolicy};
use crate::probe::{SandboxStatus, probe};
use crate::seatbelt::SeatbeltProfile;

/// Wraps a command so the kernel confines it to a policy. It only builds
/// argv; the `ProcessBackend` starts the process.
pub trait SandboxBackend: Send + Sync + std::fmt::Debug {
    /// The startup self-check's verdict, fixed for the process lifetime.
    fn status(&self) -> &SandboxStatus;

    /// The argv that runs `command` under `policy`, or why it cannot.
    fn wrap(
        &self,
        policy: &SandboxPolicy,
        command: &[String],
    ) -> Result<Vec<String>, SandboxUnavailable>;
}

/// Bash cannot run: the self-check failed (permissions.md §3.2).
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

/// macOS Seatbelt through `sandbox-exec`.
#[derive(Debug, Clone)]
pub struct Seatbelt {
    sandbox_exec: PathBuf,
    status: SandboxStatus,
}

impl Seatbelt {
    /// Runs the self-check once; the result holds for the process lifetime.
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
