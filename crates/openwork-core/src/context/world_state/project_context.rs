//! 工作目录、仓库根与顶层目录布局。
//!
//! 三个 section 里唯一高频变化的一个，因此也是本次改造收益的主要来源：它留在
//! System 前缀里时，Agent 新建一个顶层目录就会作废整段对话的缓存。
//!
//! 正文由 `context/user_project.rs` 产出，本 section 只负责比较和声明。
//!
//! **增长前提**：正文只列**顶层**条目（≤64），所以写 `src/foo.rs` 不触发变化。
//! 全量重渲染每次追加约 400 token 且不会消失，只有在这个前提下才可接受（§8.2）。
//! 该前提由 `context/user_project.rs` 的测试锁死。

use super::body::{BodyNotices, render_body_diff};
use super::{PreviousSectionState, WorldStateFragment, WorldStateSection};

const OPEN_TAG: &str = "<user_project_context";

const NOTICES: BodyNotices = BodyNotices {
    replacement: "以下项目上下文取代先前提供的项目上下文。",
    // 工作目录在 Session 内始终存在，这条实际发不出来；保留是为了让三个 section
    // 走同一套状态机，而不是给 project context 开一个特例分支。
    removal: "先前提供的项目上下文不再适用。",
};

/// 已渲染的 `<user_project_context>` 正文。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ProjectContextState {
    body: String,
}

impl ProjectContextState {
    pub(crate) fn new(body: impl Into<String>) -> Self {
        Self { body: body.into() }
    }
}

#[cfg(test)]
impl ProjectContextState {
    pub(crate) fn body_for_test(&self) -> String {
        self.body.clone()
    }
}

impl WorldStateSection for ProjectContextState {
    const ID: &'static str = "runtime/user-project-context";
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
    use super::*;

    #[test]
    fn the_section_id_matches_the_former_system_context_key() {
        assert_eq!(ProjectContextState::ID, "runtime/user-project-context");
    }

    /// 目录布局变化必须被检出。
    ///
    /// 这是本 section 存在的理由：正文里任何一个字节的变化都要让模型看到，否则
    /// 它会按一份过时的项目结构行事。
    #[test]
    fn a_new_top_level_entry_is_detected_as_a_change() {
        let previous = Some("<user_project_context>\n- src\n</user_project_context>".to_string());
        let current = "<user_project_context>\n- docs\n- src\n</user_project_context>";

        let fragment = ProjectContextState::new(current)
            .render_diff(PreviousSectionState::Known(&previous))
            .expect("a layout change emits");

        assert_eq!(
            fragment.content,
            [openwork_models::model::ContentBlock::text(format!(
                "以下项目上下文取代先前提供的项目上下文。\n\n{current}"
            ))]
        );
    }

    /// 布局没变时一个字节都不发。
    #[test]
    fn an_unchanged_layout_emits_nothing() {
        let body = "<user_project_context>\n- src\n</user_project_context>";
        let previous = Some(body.to_string());

        assert!(
            ProjectContextState::new(body)
                .render_diff(PreviousSectionState::Known(&previous))
                .is_none()
        );
    }

    /// 本 section 永远有正文，因此快照永远是 `Some`。
    ///
    /// 工作目录在 Session 内必然存在；如果哪天这里能变成 `None`，说明捕获逻辑
    /// 出了问题，而不是"这个 section 消失了"。
    #[test]
    fn the_snapshot_is_always_present() {
        assert!(
            ProjectContextState::new("<user_project_context/>")
                .snapshot()
                .is_some()
        );
    }
}
