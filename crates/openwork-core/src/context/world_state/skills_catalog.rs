//! 可用 Skill 清单。
//!
//! 正文由 `context/skill_catalog.rs` 的 `render_skill_catalog` 产出（含条数截断
//! 与告警），本 section 只负责比较和声明，不重复那套渲染。

use super::body::{BodyNotices, render_body_diff};
use super::{PreviousSectionState, WorldStateFragment, WorldStateSection};

const NOTICES: BodyNotices = BodyNotices {
    replacement: "以下 skill 清单取代先前提供的清单。",
    removal: "先前提供的 skill 清单不再适用，当前没有可用 skill。",
};

/// 已渲染的 `<available_skills>` 正文。`None` 表示没有启用中的 skill。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SkillsCatalogState {
    body: Option<String>,
}

impl SkillsCatalogState {
    pub(crate) fn new(body: Option<String>) -> Self {
        Self { body }
    }
}

#[cfg(test)]
impl SkillsCatalogState {
    pub(crate) fn body_for_test(&self) -> Option<String> {
        self.body.clone()
    }
}

impl WorldStateSection for SkillsCatalogState {
    const ID: &'static str = "skills/catalog";
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

    #[test]
    fn the_section_id_matches_the_former_system_context_key() {
        assert_eq!(SkillsCatalogState::ID, "skills/catalog");
    }

    /// 用户在设置里关掉最后一个 skill：必须告知清单失效。
    ///
    /// 不发的话模型会继续以为那些 skill 还能用，进而引用一个已经不在清单里的文件。
    #[test]
    fn disabling_every_skill_announces_that_the_catalog_no_longer_applies() {
        let previous = Some("<available_skills>\n- commit\n</available_skills>".to_string());

        let fragment = SkillsCatalogState::new(None)
            .render_diff(PreviousSectionState::Known(&previous))
            .expect("removal emits");

        assert_eq!(
            fragment.content,
            [openwork_models::model::ContentBlock::text(
                "先前提供的 skill 清单不再适用，当前没有可用 skill。"
            )]
        );
    }

    /// 清单内容变化时整份重发并声明取代，不发"新增了哪几条"。
    #[test]
    fn a_changed_catalog_is_resent_whole() {
        let previous = Some("<available_skills>\n- commit\n</available_skills>".to_string());
        let current = "<available_skills>\n- commit\n- review\n</available_skills>";

        let fragment = SkillsCatalogState::new(Some(current.to_string()))
            .render_diff(PreviousSectionState::Known(&previous))
            .expect("a change emits");

        assert_eq!(
            fragment.content,
            [openwork_models::model::ContentBlock::text(format!(
                "以下 skill 清单取代先前提供的清单。\n\n{current}"
            ))]
        );
    }
}
