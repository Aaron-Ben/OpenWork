//! 执行前的判定（permissions.md §2.1、§4、§5）：规则拒绝、出卡片，还是直接在沙箱里执行。
//!
//! 判定只看三件事：写目标是否硬保护、模型有没有请求越界、命令是否命中危险命令清单。
//! 命令会读写什么交给内核在执行时判断，这里不推断。

use openwork_sandbox::{Access, GrantScope, PathGrant, PathTier, SandboxMode, SandboxPolicy};
use openwork_tools::{DangerKey, DangerMatch, PreparedCall};
use serde::{Deserialize, Serialize};

use super::permission_state::{SessionApproval, non_interactive_denial};

/// 审批卡片的数据（permissions.md §5.1）。越界与危险命令可以同时出现在一张卡片上，
/// 一次批准覆盖两者。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApprovalCard {
    /// 调用发生时的会话模式。
    pub mode: SandboxMode,
    /// bash 的命令原文，始终完整显示。
    pub command: Option<String>,
    /// 模型给用户的一句话理由；只有越界请求才有。
    pub justification: Option<String>,
    /// 这一次额外获得的路径，逐条列出。
    pub paths: Vec<ApprovalPath>,
    pub danger: Option<ApprovalDanger>,
    /// 同一 Turn 里上一次被沙箱拒绝时的那一行输出，帮用户看清为什么要越界。
    pub previous_denial: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApprovalPath {
    /// 规范化后的绝对路径。
    pub path: String,
    pub access: Access,
    pub scope: GrantScope,
    pub tier: PathTier,
    /// 普通档里的路径在工作区内还是外（卡片上显示"工作区"或"工作区外"）。
    pub in_workspace: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApprovalDanger {
    pub key: DangerKey,
    /// 命中片段在 `command` 里的位置，按 UTF-16 码元计（界面直接用于高亮）。
    pub start: usize,
    pub end: usize,
}

/// 决定的来源（permissions.md §7 `permissionDecisionSource`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum DecisionSource {
    /// 硬保护路径或越界请求没通过校验。
    Builtin,
    /// 子 Agent 没有人可问。
    NonInteractive,
}

impl DecisionSource {
    pub(super) fn as_str(self) -> &'static str {
        match self {
            Self::Builtin => "builtin",
            Self::NonInteractive => "non_interactive",
        }
    }
}

/// 执行前的判定结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Gate {
    /// 不执行，把 `text` 作为结果还给模型；Turn 继续。
    Deny {
        text: String,
        source: DecisionSource,
    },
    /// 等用户在卡片上决定；允许后带着 `grants` 执行这一次。
    Ask {
        card: ApprovalCard,
        grants: Vec<PathGrant>,
    },
    /// 直接在会话模式下执行。
    Run,
}

pub(super) fn gate(
    prepared: &PreparedCall,
    policy: &SandboxPolicy,
    approval: SessionApproval,
    previous_denial: Option<&str>,
) -> Gate {
    if let Some(text) = &prepared.protected_target {
        return Gate::Deny {
            text: text.clone(),
            source: DecisionSource::Builtin,
        };
    }
    let danger = prepared
        .danger
        .as_ref()
        .zip(prepared.command.as_deref())
        .map(|(found, command)| danger_on_card(found, command));
    if let Some(escalation) = &prepared.escalation {
        if escalation.justification.trim().is_empty() {
            return Gate::Deny {
                text: MISSING_JUSTIFICATION.to_string(),
                source: DecisionSource::Builtin,
            };
        }
        if let Err(error) = policy.validate_grants(&escalation.grants, prepared.actor) {
            return Gate::Deny {
                text: format!("[sandbox: {error}]"),
                source: DecisionSource::Builtin,
            };
        }
        if !approval.is_interactive() {
            return Gate::Deny {
                text: non_interactive_denial(policy.mode, "extra sandbox paths"),
                source: DecisionSource::NonInteractive,
            };
        }
        return Gate::Ask {
            card: ApprovalCard {
                mode: policy.mode,
                command: prepared.command.clone(),
                justification: Some(escalation.justification.clone()),
                paths: escalation
                    .grants
                    .iter()
                    .map(|grant| card_path(grant, policy))
                    .collect(),
                danger,
                previous_denial: previous_denial.map(str::to_string),
            },
            grants: escalation.grants.clone(),
        };
    }
    // `accept-edits` 下 bash 本来就写不了工作区，危险命令会先被内核拒绝、再走越界卡片；
    // 单独再问一次只是重复（permissions.md §4.3）。
    if let Some(danger) = danger
        && policy.mode == SandboxMode::Auto
    {
        if !approval.is_interactive() {
            return Gate::Deny {
                text: non_interactive_denial(policy.mode, "a command that deletes files in bulk"),
                source: DecisionSource::NonInteractive,
            };
        }
        return Gate::Ask {
            card: ApprovalCard {
                mode: policy.mode,
                command: prepared.command.clone(),
                justification: None,
                paths: Vec::new(),
                danger: Some(danger),
                previous_denial: None,
            },
            grants: Vec::new(),
        };
    }
    Gate::Run
}

const MISSING_JUSTIFICATION: &str = "[sandbox: sandboxPermissions needs a one-sentence justification that the user will read; retry with it]";

fn card_path(grant: &PathGrant, policy: &SandboxPolicy) -> ApprovalPath {
    ApprovalPath {
        path: grant.path.to_string_lossy().into_owned(),
        access: grant.access,
        scope: grant.scope,
        tier: policy.tier(&grant.path),
        in_workspace: grant.path.starts_with(&policy.workspace_root),
    }
}

fn danger_on_card(found: &DangerMatch, command: &str) -> ApprovalDanger {
    let utf16 = |byte: usize| {
        command
            .get(..byte)
            .map_or(0, |prefix| prefix.encode_utf16().count())
    };
    ApprovalDanger {
        key: found.key,
        start: utf16(found.span.start),
        end: utf16(found.span.end),
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::Arc;

    use openwork_sandbox::{Actor, SandboxEnvironment};
    use openwork_tools::{Escalation, detect_danger};

    use super::*;

    fn policy(mode: SandboxMode) -> SandboxPolicy {
        SandboxPolicy::new(
            mode,
            PathBuf::from("/home/me/project"),
            Arc::new(SandboxEnvironment::new(
                PathBuf::from("/home/me"),
                vec![PathBuf::from("/private/tmp")],
                vec![PathBuf::from("/home/me/.agents/skills")],
            )),
        )
    }

    fn bash(command: &str) -> PreparedCall {
        PreparedCall {
            actor: Actor::Bash,
            command: Some(command.to_string()),
            escalation: None,
            danger: detect_danger(command),
            protected_target: None,
        }
    }

    fn grant(path: &str, access: Access, scope: GrantScope) -> PathGrant {
        PathGrant {
            path: PathBuf::from(path),
            access,
            scope,
        }
    }

    fn with_escalation(mut call: PreparedCall, grants: Vec<PathGrant>, why: &str) -> PreparedCall {
        call.escalation = Some(Escalation {
            grants,
            justification: why.to_string(),
        });
        call
    }

    #[test]
    fn ordinary_calls_run_without_asking() {
        assert_eq!(
            gate(
                &bash("cargo test"),
                &policy(SandboxMode::Auto),
                SessionApproval::Interactive,
                None
            ),
            Gate::Run
        );
    }

    #[test]
    fn protected_write_targets_are_rule_denials_without_a_card() {
        let call = PreparedCall {
            actor: Actor::FileTool,
            command: None,
            escalation: None,
            danger: None,
            protected_target: Some("[sandbox: .git/hooks/pre-commit is protected]".to_string()),
        };
        assert!(matches!(
            gate(
                &call,
                &policy(SandboxMode::Auto),
                SessionApproval::Interactive,
                None
            ),
            Gate::Deny {
                source: DecisionSource::Builtin,
                ..
            }
        ));
    }

    /// permissions.md §4.3：危险命令只在 `auto` 下单独出卡。
    #[test]
    fn dangerous_commands_ask_only_in_auto() {
        let call = bash("rm -rf build");
        let Gate::Ask { card, grants } = gate(
            &call,
            &policy(SandboxMode::Auto),
            SessionApproval::Interactive,
            None,
        ) else {
            panic!("auto must ask");
        };
        assert!(grants.is_empty());
        let danger = card.danger.expect("danger");
        assert_eq!((danger.start, danger.end), (0, "rm -rf build".len()));
        assert_eq!(
            gate(
                &call,
                &policy(SandboxMode::AcceptEdits),
                SessionApproval::Interactive,
                None
            ),
            Gate::Run
        );
    }

    /// permissions.md §4.2：理由为空、路径不带来新权限，都是规则拒绝。
    #[test]
    fn escalations_are_validated_before_any_card() {
        let target = "/home/me/.cargo/registry";
        let empty_reason = with_escalation(
            bash("cargo build"),
            vec![grant(target, Access::Write, GrantScope::Subtree)],
            "  ",
        );
        assert!(matches!(
            gate(
                &empty_reason,
                &policy(SandboxMode::Auto),
                SessionApproval::Interactive,
                None
            ),
            Gate::Deny { .. }
        ));
        let nothing_new = with_escalation(
            bash("touch src/x.rs"),
            vec![grant(
                "/home/me/project/src/x.rs",
                Access::Write,
                GrantScope::Exact,
            )],
            "create x",
        );
        assert!(matches!(
            gate(
                &nothing_new,
                &policy(SandboxMode::Auto),
                SessionApproval::Interactive,
                None
            ),
            Gate::Deny {
                source: DecisionSource::Builtin,
                ..
            }
        ));
    }

    #[test]
    fn a_valid_escalation_asks_with_every_path_and_its_tier() {
        let call = with_escalation(
            bash("git push -u origin main"),
            vec![
                grant("/home/me/project/.git", Access::Write, GrantScope::Subtree),
                grant("/home/me/.ssh", Access::Read, GrantScope::Subtree),
            ],
            "push the branch",
        );
        let Gate::Ask { card, grants } = gate(
            &call,
            &policy(SandboxMode::Auto),
            SessionApproval::Interactive,
            Some("error: could not lock config file .git/config: Operation not permitted"),
        ) else {
            panic!("a valid escalation asks");
        };
        assert_eq!(grants.len(), 2);
        assert_eq!(
            card.paths
                .iter()
                .map(|path| (path.tier, path.in_workspace))
                .collect::<Vec<_>>(),
            [(PathTier::Sensitive, true), (PathTier::Credential, false)]
        );
        assert_eq!(card.justification.as_deref(), Some("push the branch"));
        assert!(card.previous_denial.is_some());
    }

    /// permissions.md §9.2 #32：`accept-edits` 下 `rm -rf build` 带写工作区的越界重试时，
    /// 越界卡片同时标注危险命令，一次批准覆盖两者。
    #[test]
    fn acc_32_an_escalation_card_also_marks_a_dangerous_command() {
        let call = with_escalation(
            bash("rm -rf build"),
            vec![grant(
                "/home/me/project/build",
                Access::Write,
                GrantScope::Subtree,
            )],
            "clean the build",
        );
        let Gate::Ask { card, grants } = gate(
            &call,
            &policy(SandboxMode::AcceptEdits),
            SessionApproval::Interactive,
            None,
        ) else {
            panic!("a valid escalation asks");
        };
        assert_eq!(grants.len(), 1);
        assert_eq!(card.paths.len(), 1);
        assert_eq!(
            card.danger.map(|danger| danger.key),
            Some(DangerKey::RmRecursiveOrForce)
        );
    }

    /// permissions.md §6.6：没有人可问时，越界与危险命令都直接拒绝。
    #[test]
    fn unattended_sessions_deny_what_would_need_a_card() {
        let escalation = with_escalation(
            bash("cargo build"),
            vec![grant(
                "/home/me/.cargo/registry",
                Access::Write,
                GrantScope::Subtree,
            )],
            "fetch crates",
        );
        for call in [bash("rm -rf build"), escalation] {
            let Gate::Deny { text, source } = gate(
                &call,
                &policy(SandboxMode::Auto),
                SessionApproval::NonInteractive,
                None,
            ) else {
                panic!("unattended sessions never ask");
            };
            assert_eq!(source, DecisionSource::NonInteractive);
            assert!(text.contains("nobody can approve"), "{text}");
        }
    }

    #[test]
    fn danger_offsets_count_utf16_code_units() {
        let command = "echo 你好 && rm -rf x";
        let found = DangerMatch {
            key: DangerKey::RmRecursiveOrForce,
            span: command.find("rm").expect("rm")..command.len(),
        };
        let danger = danger_on_card(&found, command);
        assert_eq!(danger.start, "echo 你好 && ".encode_utf16().count());
    }

    /// contracts.md §3：卡片发给 Desktop 的形状，与 `desktop/src/bridge/compat.ts` 的
    /// `RuntimeApprovalCard` 一致。
    #[test]
    fn the_card_serializes_as_the_desktop_contract() {
        let card = ApprovalCard {
            mode: SandboxMode::AcceptEdits,
            command: Some("rm -rf build".to_string()),
            justification: Some("clean the build".to_string()),
            paths: vec![ApprovalPath {
                path: "/home/me/project/build".to_string(),
                access: Access::Write,
                scope: GrantScope::Subtree,
                tier: PathTier::Normal,
                in_workspace: true,
            }],
            danger: Some(ApprovalDanger {
                key: DangerKey::RmRecursiveOrForce,
                start: 0,
                end: 12,
            }),
            previous_denial: None,
        };
        assert_eq!(
            serde_json::to_value(&card).expect("card json"),
            serde_json::json!({
                "mode": "accept_edits",
                "command": "rm -rf build",
                "justification": "clean the build",
                "paths": [{
                    "path": "/home/me/project/build",
                    "access": "write",
                    "scope": "subtree",
                    "tier": "normal",
                    "inWorkspace": true
                }],
                "danger": { "key": "rm_recursive_or_force", "start": 0, "end": 12 },
                "previousDenial": null
            })
        );
    }
}
