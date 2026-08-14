//! World State：会话中会变化的上下文，以追加消息进入 Conversation。
//!
//! 分工见 `docs/research/codex-context-engineering-refactor.md` §8：System 前缀
//! 只保留 `core/agent-system` 且在 Session 内恒定；这里的三个 section 变化时
//! 才向对话末尾追加一条**全量重渲染**的消息，没变就一个字节都不发。
//!
//! 本模块只有纯逻辑：捕获正文的 IO 仍在 `context/` 下的三个 loader 里，接线在
//! 阶段二 C 完成。

use openwork_models::model::ContentBlock;

mod agents_md;
mod body;
mod project_context;
mod skills_catalog;

pub(crate) use agents_md::AgentsMdState;
pub(crate) use project_context::ProjectContextState;
pub(crate) use skills_catalog::SkillsCatalogState;

/// 上一次比较基线中某个 section 的状态。
#[derive(Debug)]
pub(crate) enum PreviousSectionState<'a, S> {
    /// 从未向模型发送过这个 section。
    Absent,
    /// 发送过，但基线不可用：进程重启或会话恢复（§9.4）。
    ///
    /// 这不是降级路径而是一等状态：section 必须按“模型可能还记得旧值”处理，
    /// 也就是带替换声明重发，而不是当作首次出现。
    Unknown,
    Known(&'a S),
}

impl<S> Clone for PreviousSectionState<'_, S> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<S> Copy for PreviousSectionState<'_, S> {}

/// 一个会随会话变化、需要让模型看见的上下文来源。
pub(crate) trait WorldStateSection {
    /// 稳定标识，同时用于识别历史里属于本 section 的消息（§9.3）。
    const ID: &'static str;

    /// 比较基线。
    ///
    /// **必须覆盖渲染读到的每一个值**（§8.1.1）。漏掉一个，那个值单独变化时
    /// 会被判成“没变”，模型永远看不到这次更新，而且不会有任何报错。
    type Snapshot: PartialEq;

    fn snapshot(&self) -> Self::Snapshot;

    /// 与基线比较后应当发给模型的内容；无变化时返回 `None`。
    fn render_diff(
        &self,
        previous: PreviousSectionState<'_, Self::Snapshot>,
    ) -> Option<WorldStateFragment>;
}

/// 一个 section 一条消息（§8.4）。
///
/// 不带 role：world-state fragment 恒为 `Role::User`（§16.2），由写入方构造，
/// 本模块不需要知道 provider 侧的表示。
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct WorldStateFragment {
    pub(crate) section_id: &'static str,
    pub(crate) content: Vec<ContentBlock>,
}

impl WorldStateFragment {
    pub(crate) fn text(section_id: &'static str, body: impl Into<String>) -> Self {
        Self {
            section_id,
            content: vec![ContentBlock::text(body)],
        }
    }
}

/// 三个 section 的当前值。
///
/// 具名字段而不是动态注册表：Codex 的类型擦除注册表是为了让扩展在运行时贡献
/// section，OpenWork 没有扩展 API，那层间接买不到任何东西（§3.2）。
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct WorldState {
    pub(crate) project_context: ProjectContextState,
    pub(crate) agents_md: AgentsMdState,
    pub(crate) skills_catalog: SkillsCatalogState,
}

/// Session actor 持有的内存基线（§8.1）。不持久化，因此恢复后一律 `Unknown`。
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct WorldStateBaseline {
    project_context: SectionBaseline<Option<String>>,
    agents_md: SectionBaseline<Option<String>>,
    skills_catalog: SectionBaseline<Option<String>>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) enum SectionBaseline<S> {
    #[default]
    Absent,
    Unknown,
    Known(S),
}

impl<S> SectionBaseline<S> {
    fn as_previous(&self) -> PreviousSectionState<'_, S> {
        match self {
            Self::Absent => PreviousSectionState::Absent,
            Self::Unknown => PreviousSectionState::Unknown,
            Self::Known(snapshot) => PreviousSectionState::Known(snapshot),
        }
    }
}

impl WorldStateBaseline {
    /// 恢复会话后的基线：见过但记不得，三个 section 都要带声明重发（§9.4）。
    pub(crate) fn unknown() -> Self {
        Self {
            project_context: SectionBaseline::Unknown,
            agents_md: SectionBaseline::Unknown,
            skills_catalog: SectionBaseline::Unknown,
        }
    }

    /// 把某个 section 的基线打回 `Absent`，用于压缩/rewind 删掉了它的消息之后
    /// 强制重发（§9.3）。
    pub(crate) fn forget(&mut self, section_id: &str) {
        match section_id {
            ProjectContextState::ID => self.project_context = SectionBaseline::Absent,
            AgentsMdState::ID => self.agents_md = SectionBaseline::Absent,
            SkillsCatalogState::ID => self.skills_catalog = SectionBaseline::Absent,
            _ => {}
        }
    }
}

impl WorldState {
    /// 对每个有差量的 section 渲染一条 fragment，并推进基线。
    ///
    /// 一次采样有几个 section 变化就返回几条，不合并（§8.4）：压缩自愈需要逐个
    /// section 判断它的消息还在不在，合成一条就只能三个一起重发。
    ///
    /// 顺序固定为 project_context → agents_md → skills_catalog，保证同样的输入
    /// 产生同样的字节。
    pub(crate) fn render_diff(&self, baseline: &mut WorldStateBaseline) -> Vec<WorldStateFragment> {
        let mut fragments = Vec::with_capacity(3);

        if let Some(fragment) = self
            .project_context
            .render_diff(baseline.project_context.as_previous())
        {
            baseline.project_context = SectionBaseline::Known(self.project_context.snapshot());
            fragments.push(fragment);
        }
        if let Some(fragment) = self.agents_md.render_diff(baseline.agents_md.as_previous()) {
            baseline.agents_md = SectionBaseline::Known(self.agents_md.snapshot());
            fragments.push(fragment);
        }
        if let Some(fragment) = self
            .skills_catalog
            .render_diff(baseline.skills_catalog.as_previous())
        {
            baseline.skills_catalog = SectionBaseline::Known(self.skills_catalog.snapshot());
            fragments.push(fragment);
        }

        fragments
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 三个 section 的 `ID` 常量要通过 trait 才能访问。
    #[allow(unused_imports)]
    use super::WorldStateSection as _;

    fn state(project: &str, agents: Option<&str>, skills: Option<&str>) -> WorldState {
        WorldState {
            project_context: ProjectContextState::new(project),
            agents_md: AgentsMdState::new(agents.map(str::to_string)),
            skills_catalog: SkillsCatalogState::new(skills.map(str::to_string)),
        }
    }

    /// 首次采样：三个 section 都发，且顺序固定。
    #[test]
    fn the_first_sampling_emits_every_section_in_a_fixed_order() {
        let mut baseline = WorldStateBaseline::default();

        let fragments =
            state("<project/>", Some("rule"), Some("<skills/>")).render_diff(&mut baseline);

        let ids: Vec<_> = fragments.iter().map(|f| f.section_id).collect();
        assert_eq!(
            ids,
            [
                ProjectContextState::ID,
                AgentsMdState::ID,
                SkillsCatalogState::ID
            ]
        );
    }

    /// 什么都没变时一条消息都不产生。
    ///
    /// 这是整个改造的收益来源：绝大多数 Model Call 不应该往历史里加任何东西。
    #[test]
    fn an_unchanged_world_emits_nothing() {
        let mut baseline = WorldStateBaseline::default();
        let world = state("<project/>", Some("rule"), Some("<skills/>"));
        world.render_diff(&mut baseline);

        assert!(world.render_diff(&mut baseline).is_empty());
    }

    /// 只有变了的 section 发消息，其余一个字节不发。
    #[test]
    fn only_the_changed_section_emits() {
        let mut baseline = WorldStateBaseline::default();
        state("<project/>", Some("rule"), Some("<skills/>")).render_diff(&mut baseline);

        let fragments =
            state("<project v=\"2\"/>", Some("rule"), Some("<skills/>")).render_diff(&mut baseline);

        let ids: Vec<_> = fragments.iter().map(|f| f.section_id).collect();
        assert_eq!(ids, [ProjectContextState::ID]);
    }

    /// 多个 section 同时变化写成多条，不合并（§8.4 / 决定 9）。
    #[test]
    fn simultaneous_changes_stay_separate_messages() {
        let mut baseline = WorldStateBaseline::default();
        state("<project/>", Some("rule"), Some("<skills/>")).render_diff(&mut baseline);

        let fragments = state("<project v=\"2\"/>", Some("rule 2"), Some("<skills/>"))
            .render_diff(&mut baseline);

        let ids: Vec<_> = fragments.iter().map(|f| f.section_id).collect();
        assert_eq!(ids, [ProjectContextState::ID, AgentsMdState::ID]);
    }

    /// 恢复会话：基线为 Unknown，三个 section 全部重发。
    #[test]
    fn an_unknown_baseline_re_emits_every_section() {
        let mut baseline = WorldStateBaseline::unknown();

        let fragments =
            state("<project/>", Some("rule"), Some("<skills/>")).render_diff(&mut baseline);

        assert_eq!(fragments.len(), 3);
    }

    /// `forget` 让指定 section 在下一次采样重发，其余不受影响（§9.3）。
    #[test]
    fn forgetting_one_section_re_emits_only_that_section() {
        let mut baseline = WorldStateBaseline::default();
        let world = state("<project/>", Some("rule"), Some("<skills/>"));
        world.render_diff(&mut baseline);

        baseline.forget(AgentsMdState::ID);
        let fragments = world.render_diff(&mut baseline);

        let ids: Vec<_> = fragments.iter().map(|f| f.section_id).collect();
        assert_eq!(ids, [AgentsMdState::ID]);
    }

    /// 未知 section_id 不能静默改动基线。
    #[test]
    fn forgetting_an_unknown_section_changes_nothing() {
        let mut baseline = WorldStateBaseline::default();
        let world = state("<project/>", Some("rule"), Some("<skills/>"));
        world.render_diff(&mut baseline);
        let before = baseline.clone();

        baseline.forget("runtime/not-a-section");

        assert_eq!(baseline, before);
    }
}
