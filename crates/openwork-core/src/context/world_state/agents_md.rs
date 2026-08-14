//! 项目 `AGENTS.md`。
//!
//! 它原先是 System 前缀的一段，现在下沉为 world-state section：Agent 自己就能
//! 编辑这个文件，留在前缀里意味着整个前缀一直在担缓存失效的风险（§16.2）。
//!
//! # 为什么要包一层 `<project_instructions>`
//!
//! 另外两个 section 的正文自带标记（`<user_project_context>`、`<available_skills>`），
//! 只有这里是一份用户写的自由文本，必须自己补上，理由有两条：
//!
//! 1. **role 不再承担区分职责。** 它原先在 System role 里，role 本身就划清了
//!    "项目规范"与"用户说的话"。§16.2 决定改用 `Role::User` 之后，一条 user
//!    消息里躺着一份文件内容，不加标记模型无从判断这是项目规范还是用户刚打的字。
//! 2. **压缩自愈要认得它。** §9.3 要判断"某 section 的消息还在不在投影里"，
//!    `message_kind` 只能说明"这是条 world-state 消息"，说不出是哪个 section 的。
//!
//! Codex 出于同样的理由包了 `<INSTRUCTIONS>`（`core/src/context/user_instructions.rs`）。

use super::body::{BodyNotices, render_body_diff};
use super::{PreviousSectionState, WorldStateFragment, WorldStateSection};

const OPEN_TAG: &str = "<project_instructions>";
const CLOSE_TAG: &str = "</project_instructions>";

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

    /// 带标记的正文。
    ///
    /// **比较和渲染必须都走这里。** 一边用裸文本一边用带标记的，比较就会每次都
    /// 判成"变了"，于是每个 Model Call 都重发一份 AGENTS.md——正好把这次改造想
    /// 省的东西反向放大。
    fn wrapped(&self) -> Option<String> {
        self.body
            .as_ref()
            .map(|body| format!("{OPEN_TAG}\n{body}\n{CLOSE_TAG}"))
    }
}

impl WorldStateSection for AgentsMdState {
    const ID: &'static str = "project/AGENTS.md";
    type Snapshot = Option<String>;

    fn snapshot(&self) -> Self::Snapshot {
        self.wrapped()
    }

    fn render_diff(
        &self,
        previous: PreviousSectionState<'_, Self::Snapshot>,
    ) -> Option<WorldStateFragment> {
        render_body_diff(Self::ID, NOTICES, self.wrapped().as_deref(), previous)
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

    /// 文件内容原样保留，外面套一层标记。
    ///
    /// 内容一个字都不能改写——`AGENTS.md` 是用户写的规范文本。但外层标记是必需
    /// 的：改用 `Role::User` 之后，没有标记就无法把项目规范与用户输入区分开。
    #[test]
    fn the_body_is_wrapped_but_its_content_is_untouched() {
        let fragment = AgentsMdState::new(Some("永远先跑测试".to_string()))
            .render_diff(PreviousSectionState::Absent)
            .expect("first appearance emits");

        assert_eq!(
            fragment.content,
            [openwork_models::model::ContentBlock::text(
                "<project_instructions>\n永远先跑测试\n</project_instructions>"
            )]
        );
    }

    /// 比较和渲染必须用同一种形态。
    ///
    /// 快照存裸文本而渲染带标记的话，两者永远不相等，于是每个 Model Call 都重发
    /// 一份 AGENTS.md。这种 bug 不会报错，只会让账单变大。
    #[test]
    fn an_unchanged_file_still_compares_equal_after_wrapping() {
        let state = AgentsMdState::new(Some("永远先跑测试".to_string()));
        let previous = state.snapshot();

        assert!(
            state
                .render_diff(PreviousSectionState::Known(&previous))
                .is_none()
        );
    }

    /// 编辑后重发时带取代声明，标记在声明之内。
    #[test]
    fn an_edit_is_announced_as_a_replacement() {
        let previous = AgentsMdState::new(Some("旧规范".to_string())).snapshot();

        let fragment = AgentsMdState::new(Some("新规范".to_string()))
            .render_diff(PreviousSectionState::Known(&previous))
            .expect("an edit emits");

        assert_eq!(
            fragment.content,
            [openwork_models::model::ContentBlock::text(
                "以下 AGENTS.md 指令取代先前提供的全部 AGENTS.md 指令。\n\n\
                 <project_instructions>\n新规范\n</project_instructions>"
            )]
        );
    }

    /// 每一条 fragment 都必须能被认出属于本 section（§9.3 的前提）。
    ///
    /// 正常正文靠标记，失效声明靠它自己是个固定串。两者都是本 section 已知的
    /// 常量，压缩自愈才有得判。
    #[test]
    fn every_fragment_is_identifiable_as_this_section() {
        let present = AgentsMdState::new(Some("规范".to_string()))
            .render_diff(PreviousSectionState::Absent)
            .expect("present emits");
        let previous = AgentsMdState::new(Some("规范".to_string())).snapshot();
        let removed = AgentsMdState::new(None)
            .render_diff(PreviousSectionState::Known(&previous))
            .expect("removal emits");

        let text = |fragment: &WorldStateFragment| match fragment.content.as_slice() {
            [openwork_models::model::ContentBlock::Text(block)] => block.text.clone(),
            other => panic!("expected one text block, got {other:?}"),
        };

        assert!(text(&present).contains("<project_instructions>"));
        assert_eq!(text(&removed), "先前提供的 AGENTS.md 指令不再适用。");
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
