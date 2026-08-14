//! 项目 `AGENTS.md`。
//!
//! 它原先是 System 前缀的一段，现在下沉为 world-state section：Agent 自己就能
//! 编辑这个文件，留在前缀里意味着整个前缀一直在担缓存失效的风险（§16.2）。

use super::body::{BodyNotices, render_body_diff};
use super::{PreviousSectionState, WorldStateFragment, WorldStateSection};

const NOTICES: BodyNotices = BodyNotices {
    replacement: "以下 AGENTS.md 指令取代先前提供的全部 AGENTS.md 指令。",
    removal: "先前提供的 AGENTS.md 指令不再适用。",
};

/// 当前模型可见的 `AGENTS.md` 正文。`None` 表示文件不存在或内容为空。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AgentsMdState {
    body: Option<String>,
}

impl AgentsMdState {
    pub(crate) fn new(body: Option<String>) -> Self {
        Self { body }
    }
}

impl WorldStateSection for AgentsMdState {
    const ID: &'static str = "project/AGENTS.md";
    type Snapshot = Option<String>;

    fn snapshot(&self) -> Self::Snapshot {
        self.body.clone()
    }

    fn render_diff(
        &self,
        previous: PreviousSectionState<'_, Self::Snapshot>,
    ) -> Option<WorldStateFragment> {
        render_body_diff(Self::ID, NOTICES, self.body.as_deref(), previous)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 稳定标识沿用原 System 片段的 key。
    ///
    /// 它同时是历史里识别本 section 消息的依据（§9.3），改它等于让所有已发出的
    /// 消息失去归属，因此锁死。
    #[test]
    fn the_section_id_matches_the_former_system_context_key() {
        assert_eq!(AgentsMdState::ID, "project/AGENTS.md");
    }

    /// 快照就是渲染读到的全部内容，因此正文一变快照必变（§8.1.1）。
    #[test]
    fn the_snapshot_covers_the_rendered_body() {
        let before = AgentsMdState::new(Some("旧".to_string())).snapshot();
        let after = AgentsMdState::new(Some("新".to_string())).snapshot();

        assert_ne!(before, after);
    }

    /// 正文原样进入消息，不做包装或改写。
    ///
    /// `AGENTS.md` 是用户写的规范文本，任何额外包装都会改变它的语义边界。
    #[test]
    fn the_body_reaches_the_model_verbatim() {
        let fragment = AgentsMdState::new(Some("永远先跑测试".to_string()))
            .render_diff(PreviousSectionState::Absent)
            .expect("first appearance emits");

        assert_eq!(
            fragment.content,
            [openwork_models::model::ContentBlock::text("永远先跑测试")]
        );
    }

    /// 编辑后重发时带取代声明。
    #[test]
    fn an_edit_is_announced_as_a_replacement() {
        let previous = Some("旧规范".to_string());

        let fragment = AgentsMdState::new(Some("新规范".to_string()))
            .render_diff(PreviousSectionState::Known(&previous))
            .expect("an edit emits");

        assert_eq!(
            fragment.content,
            [openwork_models::model::ContentBlock::text(
                "以下 AGENTS.md 指令取代先前提供的全部 AGENTS.md 指令。\n\n新规范"
            )]
        );
    }

    /// 文件被删除时明确告知失效，而不是悄悄不发。
    #[test]
    fn deleting_the_file_tells_the_model_the_rules_no_longer_apply() {
        let previous = Some("旧规范".to_string());

        let fragment = AgentsMdState::new(None)
            .render_diff(PreviousSectionState::Known(&previous))
            .expect("removal emits");

        assert_eq!(
            fragment.content,
            [openwork_models::model::ContentBlock::text(
                "先前提供的 AGENTS.md 指令不再适用。"
            )]
        );
    }
}
