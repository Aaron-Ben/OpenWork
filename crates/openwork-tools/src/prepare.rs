//! 执行前 Core 需要知道的事实（permissions.md §2.1）。
//!
//! 工具只报告事实——命令原文、写目标、模型请求的越界；要不要问、问什么由 Core 决定。
//! 这里不推断命令会读写什么：那由内核在执行时判断。

use openwork_sandbox::{Actor, PathGrant, PathTier, SandboxPolicy};

use crate::ToolSessionContext;
use crate::escalation::{Escalation, EscalationInput};
use crate::notice;
use crate::path::display_path;
use crate::permission::{DangerMatch, detect_danger};

/// 一个工具从自己的输入里报告的事实。
#[derive(Debug, Clone)]
pub struct CallInspection {
    actor: Actor,
    command: Option<String>,
    write_target: Option<String>,
    escalation: EscalationInput,
}

impl CallInspection {
    /// 不写文件、不启动进程的工具（read、grep、glob、list）。
    pub fn read_only() -> Self {
        Self {
            actor: Actor::FileTool,
            command: None,
            write_target: None,
            escalation: EscalationInput::default(),
        }
    }

    /// 写一个路径的文件工具（write、edit）。
    pub fn writes(target: impl Into<String>, escalation: EscalationInput) -> Self {
        Self {
            write_target: Some(target.into()),
            escalation,
            ..Self::read_only()
        }
    }

    /// 在沙箱里运行一条 shell 命令（bash）。
    pub fn runs(command: impl Into<String>, escalation: EscalationInput) -> Self {
        Self {
            actor: Actor::Bash,
            command: Some(command.into()),
            write_target: None,
            escalation,
        }
    }
}

/// Core 判定一次调用所需的全部事实。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreparedCall {
    /// 越界校验按哪一方的可写范围判断"是否带来新权限"。
    pub actor: Actor,
    /// bash 的命令原文，卡片上完整显示。
    pub command: Option<String>,
    /// 模型请求的越界，路径已规范化。沙箱不可用时总是 `None`（参数不在 schema 里）。
    pub escalation: Option<Escalation>,
    /// 命中的危险命令（只有 bash 有）。
    pub danger: Option<DangerMatch>,
    /// 写目标是硬保护路径时给模型的规则拒绝文本：不调用工具、不出卡片。
    pub protected_target: Option<String>,
}

pub(crate) async fn prepare(
    session: &ToolSessionContext,
    inspection: CallInspection,
    policy: &SandboxPolicy,
) -> PreparedCall {
    let CallInspection {
        actor,
        command,
        write_target,
        escalation,
    } = inspection;
    let escalation = match escalation.sandbox_permissions {
        Some(permissions) if session.escalation_available() => {
            let mut grants = Vec::with_capacity(permissions.paths.len());
            for path in permissions.paths {
                grants.push(PathGrant {
                    path: session.canonical_grant_path(&path.path).await,
                    access: path.access.access(),
                    scope: path.scope.scope(),
                });
            }
            Some(Escalation {
                grants,
                justification: escalation.justification.unwrap_or_default(),
            })
        }
        _ => None,
    };
    let protected_target = match write_target {
        Some(target) => {
            let actual = session.canonical_or_lexical(&target).await;
            (policy.tier(&actual) == PathTier::HardProtected)
                .then(|| notice::protected(&display_path(&policy.workspace_root, &actual)))
        }
        None => None,
    };
    // 沙箱不可用时 bash 不会执行（permissions.md §3.2、§4.3），为它出危险命令卡片只会让用户白问一次。
    let danger = if session.escalation_available() {
        command.as_deref().and_then(detect_danger)
    } else {
        None
    };
    PreparedCall {
        actor,
        command,
        escalation,
        danger,
        protected_target,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{policy_for, unavailable_session, unconfined_session};

    /// permissions.md §9.2 #13：沙箱不可用时 bash 不执行，危险命令也不出卡片。
    #[tokio::test]
    async fn an_unavailable_sandbox_reports_no_dangerous_command() {
        let workspace = tempfile::tempdir().expect("workspace");
        let policy = policy_for(workspace.path());
        let command = || CallInspection::runs("rm -rf build", EscalationInput::default());

        let available = prepare(&unconfined_session(workspace.path()), command(), &policy).await;
        let unavailable = prepare(&unavailable_session(workspace.path()), command(), &policy).await;

        assert_eq!(
            available.danger.map(|found| found.key),
            Some(crate::DangerKey::RmRecursiveOrForce)
        );
        assert_eq!(unavailable.danger, None);
        assert_eq!(unavailable.command.as_deref(), Some("rm -rf build"));
    }
}
