//! 当前的沙箱策略（permissions.md §11）：模式、工作区根、bash 是否可用。
//!
//! 放在 world state 而不是系统提示词里，理由有两条：模式可以在会话中途切换，写进前缀
//! 会作废缓存；DSH 的实测表明事先声明"只读沙箱"会让模型直接放弃工作，所以这里只陈述
//! 事实，边界由工具结果里的拒绝标记在相关时刻指出。
//!
//! 这个 section 在会话里始终存在，失效声明实际发不出来；保留它是为了与另外三个
//! section 走同一套状态机。

use openwork_sandbox::{SandboxMode, SandboxPolicy};

use super::body::{BodyNotices, render_body_diff};
use super::{PreviousSectionState, WorldStateFragment, WorldStateSection};

const OPEN_TAG: &str = "<sandbox_policy>";
const CLOSE_TAG: &str = "</sandbox_policy>";

const NOTICES: BodyNotices = BodyNotices {
    replacement: "The sandbox policy below replaces the previous one.",
    removal: "The previous sandbox policy no longer applies.",
};

/// bash 不可用时给模型的说明（permissions.md §6）。
const BASH_UNAVAILABLE: &str = "bash: unavailable, because the macOS sandbox failed its self-check. Use read / grep / glob / edit for file work, and tell the user which command you need them to run.";

/// 渲染好的 `<sandbox_policy>` 正文。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SandboxPolicyState {
    body: String,
}

impl SandboxPolicyState {
    /// `policy` 是会话模式下、不带越界的策略；`bash_available` 是启动自检的结论。
    pub(crate) fn new(policy: &SandboxPolicy, bash_available: bool) -> Self {
        Self {
            body: render(policy, bash_available),
        }
    }
}

#[cfg(test)]
impl SandboxPolicyState {
    /// `/Users/me/project` 下 `auto` 模式、沙箱可用时的正文。
    pub(crate) fn auto_for_test() -> Self {
        Self::new(
            &SandboxPolicy::new(
                SandboxMode::Auto,
                std::path::PathBuf::from("/Users/me/project"),
                std::sync::Arc::new(openwork_sandbox::SandboxEnvironment::new(
                    std::path::PathBuf::from("/Users/me"),
                    vec![std::path::PathBuf::from("/private/tmp")],
                    Vec::new(),
                )),
            ),
            true,
        )
    }
}

fn render(policy: &SandboxPolicy, bash_available: bool) -> String {
    let mode = match policy.mode {
        SandboxMode::Auto => "auto",
        SandboxMode::AcceptEdits => "accept-edits",
    };
    let bash = if !bash_available {
        BASH_UNAVAILABLE
    } else if policy.bash_writes_workspace() {
        "bash: runs in the sandbox and can write the workspace and temporary directories."
    } else {
        "bash: runs in the sandbox and can write temporary directories; writes to the workspace need sandboxPermissions."
    };
    format!(
        "{OPEN_TAG}\nmode: {mode}\nworkspace: {workspace}\nwrite / edit: can change files in the workspace.\n{bash}\n{CLOSE_TAG}",
        workspace = policy.workspace_root.display(),
    )
}

impl WorldStateSection for SandboxPolicyState {
    const ID: &'static str = "runtime/sandbox-policy";
    type Snapshot = Option<String>;

    fn snapshot(&self) -> Self::Snapshot {
        Some(self.body.clone())
    }

    fn render_diff(
        &self,
        previous: PreviousSectionState<'_, Self::Snapshot>,
    ) -> Option<WorldStateFragment> {
        render_body_diff(Self::ID, NOTICES, Some(self.body.as_str()), previous)
    }

    fn matches(text: &str) -> bool {
        text.starts_with(OPEN_TAG)
            || text
                .strip_prefix(NOTICES.replacement)
                .and_then(|body| body.strip_prefix("\n\n"))
                .is_some_and(|body| body.starts_with(OPEN_TAG))
            || text == NOTICES.removal
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::Arc;

    use openwork_models::model::ContentBlock;
    use openwork_sandbox::SandboxEnvironment;

    use super::*;

    fn policy(mode: SandboxMode, workspace: &str) -> SandboxPolicy {
        SandboxPolicy::new(
            mode,
            PathBuf::from(workspace),
            Arc::new(SandboxEnvironment::new(
                PathBuf::from("/Users/me"),
                vec![PathBuf::from("/private/tmp")],
                Vec::new(),
            )),
        )
    }

    fn text(state: &SandboxPolicyState) -> String {
        let fragment = state
            .render_diff(PreviousSectionState::Absent)
            .expect("first appearance emits");
        match fragment.content.as_slice() {
            [ContentBlock::Text(block)] => block.text.clone(),
            other => panic!("expected one text block, got {other:?}"),
        }
    }

    /// permissions.md §15 #38：模式、工作区根与 bash 能写什么，逐字。
    #[test]
    fn auto_mode_tells_the_model_bash_can_write_the_workspace() {
        let state = SandboxPolicyState::new(&policy(SandboxMode::Auto, "/Users/me/project"), true);
        assert_eq!(
            text(&state),
            "<sandbox_policy>\nmode: auto\nworkspace: /Users/me/project\nwrite / edit: can change files in the workspace.\nbash: runs in the sandbox and can write the workspace and temporary directories.\n</sandbox_policy>"
        );
    }

    #[test]
    fn accept_edits_tells_the_model_workspace_writes_from_bash_need_escalation() {
        let state =
            SandboxPolicyState::new(&policy(SandboxMode::AcceptEdits, "/Users/me/project"), true);
        assert!(text(&state).contains(
            "\nbash: runs in the sandbox and can write temporary directories; writes to the workspace need sandboxPermissions.\n"
        ));
    }

    /// permissions.md §2：工作区包含主目录时，`auto` 下 bash 也写不了工作区。
    #[test]
    fn a_home_workspace_is_described_as_not_writable_by_bash_even_in_auto() {
        let state = SandboxPolicyState::new(&policy(SandboxMode::Auto, "/Users/me"), true);
        assert!(text(&state).contains("writes to the workspace need sandboxPermissions"));
    }

    /// permissions.md §15 #13：沙箱不可用时写明 bash 不可用。
    #[test]
    fn an_unavailable_sandbox_says_bash_is_unavailable() {
        let state = SandboxPolicyState::new(&policy(SandboxMode::Auto, "/Users/me/project"), false);
        assert!(text(&state).contains(&format!("\n{BASH_UNAVAILABLE}\n")));
    }

    /// 模式切换后追加一条带取代声明的新快照；没变就不发。
    #[test]
    fn a_mode_change_is_announced_and_an_unchanged_policy_is_silent() {
        let auto = SandboxPolicyState::new(&policy(SandboxMode::Auto, "/Users/me/project"), true);
        let before = auto.snapshot();
        assert!(
            auto.render_diff(PreviousSectionState::Known(&before))
                .is_none()
        );

        let edits =
            SandboxPolicyState::new(&policy(SandboxMode::AcceptEdits, "/Users/me/project"), true);
        let fragment = edits
            .render_diff(PreviousSectionState::Known(&before))
            .expect("a changed mode emits");
        let [ContentBlock::Text(block)] = fragment.content.as_slice() else {
            panic!("one text block");
        };
        assert!(block.text.starts_with(NOTICES.replacement));
        assert!(SandboxPolicyState::matches(&block.text));
    }
}
