//! World State：会话中会变化的上下文，以追加消息进入 Conversation。
//!
//! 分工见 `docs/research/codex-context-engineering-refactor.md` §8：System 前缀
//! 只保留 `core/agent-system` 且在 Session 内恒定；这里的三个 section 变化时
//! 才向对话末尾追加一条**全量重渲染**的消息，没变就一个字节都不发。
//!
//! 本模块只有纯逻辑：捕获正文的 IO 仍在 `context/` 下的三个 loader 里，接线在
//! 阶段二 C 完成。

use openwork_chat_state::{ConversationItem, MessageKind};
use openwork_models::model::ContentBlock;

mod agents_md;
mod body;
mod capture;
mod project_context;
mod skills_catalog;

pub(crate) use agents_md::AgentsMdState;
pub(crate) use capture::{WorldStateCapture, WorldStateCaptureError};
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
    ///
    /// 它是**推导**出来的，不是存下来的——见 `SectionBaseline::as_previous`。
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

    /// 这段正文是不是本 section 发出去的。
    ///
    /// §9.3 的自愈要判断"某 section 的消息还在不在投影里"。`message_kind` 只能
    /// 说明"这是条 world-state 消息"，说不出是哪个 section 的，因此还要靠正文。
    ///
    /// **两种 fragment 都要认得**：带标记的正常正文，以及失效声明那个固定串。
    /// 只认前者的话，一个已经消失的 section 会被判成"消息不在了"，于是每轮重发
    /// 一遍失效声明。
    fn matches(text: &str) -> bool;
}

/// 当前投影里还留着哪些 section 的消息。
///
/// 压缩和 rewind 都会把历史尾部换掉，被换走的 fragment 等于模型再也看不到了。
/// 这个扫描是自愈的输入（§9.3）。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct RetainedSections {
    pub(crate) project_context: bool,
    pub(crate) agents_md: bool,
    pub(crate) skills_catalog: bool,
}

impl RetainedSections {
    /// 扫描投影。只看 `MessageKind::WorldState` 的消息。
    ///
    /// 靠 kind 先筛是必需的：用户完全可能自己打一段包含 `<available_skills>`
    /// 的文本，那不是本 section 发的，不能算数。
    pub(crate) fn scan(items: &[ConversationItem]) -> Self {
        let mut retained = Self::default();

        for text in items
            .iter()
            .filter(|item| item.kind == MessageKind::WorldState)
            .flat_map(|item| &item.message.content)
            .filter_map(|block| match block {
                ContentBlock::Text(block) => Some(block.text.as_str()),
                _ => None,
            })
        {
            retained.project_context |= ProjectContextState::matches(text);
            retained.agents_md |= AgentsMdState::matches(text);
            retained.skills_catalog |= SkillsCatalogState::matches(text);
        }

        retained
    }
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

/// Session actor 持有的内存基线（§8.1）。不持久化；恢复后如何取舍见
/// `SectionBaseline::as_previous` 的表。
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct WorldStateBaseline {
    project_context: SectionBaseline<Option<String>>,
    agents_md: SectionBaseline<Option<String>>,
    skills_catalog: SectionBaseline<Option<String>>,
}

/// 某个 section 的内存基线。
///
/// 没有 `Unknown`：那是**推导出来的**状态，不是存下来的。基线为空而投影里还有
/// 该 section 的消息，才是 `Unknown`（§9.3 的表）。
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) enum SectionBaseline<S> {
    #[default]
    Absent,
    Known(S),
}

impl<S> SectionBaseline<S> {
    /// 按 §9.3 的表，结合"消息是否还在投影里"决定 previous。
    ///
    /// | 基线 | 投影里有 | previous | 效果 |
    /// |---|---|---|---|
    /// | 有 | 有 | `Known` | 正常比较，没变就不发 |
    /// | 有 | 没有 | `Absent` | 被压缩删了，重发且不带取代声明 |
    /// | 无 | 有 | `Unknown` | 重启，重发且**带**取代声明 |
    /// | 无 | 没有 | `Absent` | 从未发过，当前也缺失就什么都不发 |
    ///
    /// 最后一行是必须的：只按"基线为空"判 `Unknown` 的话，一个既没有
    /// `AGENTS.md` 也没有 skill 的仓库，每次恢复都会为从未提供过的东西发两条
    /// "不再适用"。
    fn as_previous(&self, retained: bool) -> PreviousSectionState<'_, S> {
        match (self, retained) {
            (Self::Known(snapshot), true) => PreviousSectionState::Known(snapshot),
            (Self::Known(_), false) => PreviousSectionState::Absent,
            (Self::Absent, true) => PreviousSectionState::Unknown,
            (Self::Absent, false) => PreviousSectionState::Absent,
        }
    }
}

impl WorldStateBaseline {
    /// 在 fragment **全部落库成功之后**推进基线。
    ///
    /// 与 `WorldState::render_diff` 分开是必需的（§8.3）：渲染时就推进的话，落库
    /// 失败会留下"内存认为模型已经看过、库里却没有这条消息"的状态，那次更新从此
    /// 永久丢失，而且不会有任何报错。顺序只能是先落库、再推进。
    ///
    /// 只推进真的产生了 fragment 的 section，其余保持原样。
    pub(crate) fn advance(&mut self, world: &WorldState, fragments: &[WorldStateFragment]) {
        for fragment in fragments {
            match fragment.section_id {
                ProjectContextState::ID => {
                    self.project_context = SectionBaseline::Known(world.project_context.snapshot());
                }
                AgentsMdState::ID => {
                    self.agents_md = SectionBaseline::Known(world.agents_md.snapshot());
                }
                SkillsCatalogState::ID => {
                    self.skills_catalog = SectionBaseline::Known(world.skills_catalog.snapshot());
                }
                _ => {}
            }
        }
    }
}

impl WorldState {
    /// 对每个有差量的 section 渲染一条 fragment。**不推进基线**。
    ///
    /// 推进由 `WorldStateBaseline::advance` 在落库成功之后单独完成，理由见那里。
    ///
    /// 一次采样有几个 section 变化就返回几条，不合并（§8.4）：压缩自愈需要逐个
    /// section 判断它的消息还在不在，合成一条就只能三个一起重发。
    ///
    /// 顺序固定为 project_context → agents_md → skills_catalog，保证同样的输入
    /// 产生同样的字节。
    pub(crate) fn render_diff(
        &self,
        baseline: &WorldStateBaseline,
        retained: RetainedSections,
    ) -> Vec<WorldStateFragment> {
        [
            self.project_context.render_diff(
                baseline
                    .project_context
                    .as_previous(retained.project_context),
            ),
            self.agents_md
                .render_diff(baseline.agents_md.as_previous(retained.agents_md)),
            self.skills_catalog
                .render_diff(baseline.skills_catalog.as_previous(retained.skills_catalog)),
        ]
        .into_iter()
        .flatten()
        .collect()
    }
}

#[cfg(test)]
mod tests {
    use openwork_models::model::{Message, Role};

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

    /// 投影里三个 section 的消息都还在。
    fn all_retained() -> RetainedSections {
        RetainedSections {
            project_context: true,
            agents_md: true,
            skills_catalog: true,
        }
    }

    /// 完整走一次采样：渲染 → 落库成功 → 推进基线。
    fn sample(
        world: &WorldState,
        baseline: &mut WorldStateBaseline,
        retained: RetainedSections,
    ) -> Vec<WorldStateFragment> {
        let fragments = world.render_diff(baseline, retained);
        baseline.advance(world, &fragments);
        fragments
    }

    fn ids(fragments: &[WorldStateFragment]) -> Vec<&'static str> {
        fragments.iter().map(|f| f.section_id).collect()
    }

    fn text_of(fragment: &WorldStateFragment) -> String {
        match fragment.content.as_slice() {
            [ContentBlock::Text(block)] => block.text.clone(),
            other => panic!("expected one text block, got {other:?}"),
        }
    }

    fn world_state_item(text: &str) -> ConversationItem {
        ConversationItem::real_with_kind(Message::text(Role::User, text), MessageKind::WorldState)
    }

    /// 首次采样：三个 section 都发，且顺序固定。
    #[test]
    fn the_first_sampling_emits_every_section_in_a_fixed_order() {
        let mut baseline = WorldStateBaseline::default();

        let fragments = sample(
            &state("<project/>", Some("rule"), Some("<skills/>")),
            &mut baseline,
            RetainedSections::default(),
        );

        assert_eq!(
            ids(&fragments),
            [
                ProjectContextState::ID,
                AgentsMdState::ID,
                SkillsCatalogState::ID
            ]
        );
    }

    /// 什么都没变、消息也都还在时，一条都不产生。
    #[test]
    fn an_unchanged_world_emits_nothing() {
        let mut baseline = WorldStateBaseline::default();
        let world = state("<project/>", Some("rule"), Some("<skills/>"));
        sample(&world, &mut baseline, RetainedSections::default());

        assert!(sample(&world, &mut baseline, all_retained()).is_empty());
    }

    /// 只有变了的 section 发消息。
    #[test]
    fn only_the_changed_section_emits() {
        let mut baseline = WorldStateBaseline::default();
        sample(
            &state("<project/>", Some("rule"), Some("<skills/>")),
            &mut baseline,
            RetainedSections::default(),
        );

        let fragments = sample(
            &state("<project v=\"2\"/>", Some("rule"), Some("<skills/>")),
            &mut baseline,
            all_retained(),
        );

        assert_eq!(ids(&fragments), [ProjectContextState::ID]);
    }

    /// 多个 section 同时变化写成多条，不合并（§8.4）。
    #[test]
    fn simultaneous_changes_stay_separate_messages() {
        let mut baseline = WorldStateBaseline::default();
        sample(
            &state("<project/>", Some("rule"), Some("<skills/>")),
            &mut baseline,
            RetainedSections::default(),
        );

        let fragments = sample(
            &state("<project v=\"2\"/>", Some("rule 2"), Some("<skills/>")),
            &mut baseline,
            all_retained(),
        );

        assert_eq!(
            ids(&fragments),
            [ProjectContextState::ID, AgentsMdState::ID]
        );
    }

    /// 渲染本身不推进基线（§8.3）。
    #[test]
    fn rendering_alone_does_not_advance_the_baseline() {
        let mut baseline = WorldStateBaseline::default();
        let world = state("<project/>", Some("rule"), Some("<skills/>"));
        let retained = RetainedSections::default();

        let first = world.render_diff(&baseline, retained);
        assert_eq!(first.len(), 3);
        let retried = world.render_diff(&baseline, retained);
        assert_eq!(retried, first);

        baseline.advance(&world, &retried);
        assert!(world.render_diff(&baseline, all_retained()).is_empty());
    }

    /// 只推进真的发出去了的 section。
    #[test]
    fn advancing_only_touches_the_sections_that_emitted() {
        let mut baseline = WorldStateBaseline::default();
        let world = state("<project/>", Some("rule"), Some("<skills/>"));
        let fragments = world.render_diff(&baseline, RetainedSections::default());

        let project_only: Vec<_> = fragments
            .into_iter()
            .filter(|fragment| fragment.section_id == ProjectContextState::ID)
            .collect();
        baseline.advance(&world, &project_only);

        let remaining = world.render_diff(&baseline, all_retained());
        assert_eq!(ids(&remaining), [AgentsMdState::ID, SkillsCatalogState::ID]);
    }

    // ---- §9.3 的四象限 ----

    /// 压缩删掉了某 section 的消息 → 重发全量，且**不带**取代声明。
    ///
    /// 不带声明是对的：模型的上下文里已经没有那份旧内容了，声称"取代先前提供的"
    /// 会指向一个它看不见的东西。
    #[test]
    fn a_compacted_away_fragment_is_re_emitted_as_a_first_appearance() {
        let mut baseline = WorldStateBaseline::default();
        let world = state("<project/>", Some("规范"), Some("<skills/>"));
        sample(&world, &mut baseline, RetainedSections::default());

        let fragments = world.render_diff(
            &baseline,
            RetainedSections {
                agents_md: false,
                ..all_retained()
            },
        );

        assert_eq!(ids(&fragments), [AgentsMdState::ID]);
        assert_eq!(
            text_of(&fragments[0]),
            "<project_instructions>\n规范\n</project_instructions>",
            "被压缩删掉之后重发，不该带取代声明"
        );
    }

    /// 进程重启：基线为空但投影里还有消息 → 重发全量，且**带**取代声明。
    ///
    /// 当作首次出现是错的——模型上下文里还躺着上一份，不声明取代就会同时存在
    /// 两份互相矛盾的项目规范。
    #[test]
    fn a_restarted_session_re_emits_with_a_replacement_notice() {
        let world = state("<project/>", Some("规范"), Some("<skills/>"));

        let fragments = world.render_diff(&WorldStateBaseline::default(), all_retained());

        assert_eq!(fragments.len(), 3);
        let agents = fragments
            .iter()
            .find(|fragment| fragment.section_id == AgentsMdState::ID)
            .expect("agents md");
        assert!(
            text_of(agents).starts_with("以下 AGENTS.md 指令取代先前提供的"),
            "重启后重发必须声明取代"
        );
    }

    /// 全新会话：基线为空、投影里也没有 → 缺失的 section 什么都不发。
    ///
    /// 这条守的是一个具体的坑：只按"基线为空"判 `Unknown` 的话，一个既没有
    /// `AGENTS.md` 也没有 skill 的仓库，每次新建会话都会为从未提供过的东西
    /// 发两条"不再适用"。
    #[test]
    fn a_fresh_session_stays_silent_about_sections_that_never_existed() {
        let world = state("<project/>", None, None);

        let fragments =
            world.render_diff(&WorldStateBaseline::default(), RetainedSections::default());

        assert_eq!(ids(&fragments), [ProjectContextState::ID]);
    }

    // ---- 投影扫描 ----

    /// 扫描认得三个 section 各自的消息。
    #[test]
    fn the_scan_recognizes_each_section() {
        let retained = RetainedSections::scan(&[
            world_state_item(
                "<user_project_context format_version=\"1\">\n</user_project_context>",
            ),
            world_state_item("<project_instructions>\n规范\n</project_instructions>"),
            world_state_item("<available_skills>\n</available_skills>"),
        ]);

        assert_eq!(retained, all_retained());
    }

    /// 失效声明也算这个 section 的消息。
    ///
    /// 不认它的话，一个已经消失的 section 会被判成"消息不在了"，于是每轮重发
    /// 一遍失效声明。
    #[test]
    fn a_removal_notice_still_counts_as_the_sections_message() {
        let retained =
            RetainedSections::scan(&[world_state_item("先前提供的 AGENTS.md 指令不再适用。")]);

        assert!(retained.agents_md);
        assert!(!retained.project_context);
    }

    /// 用户自己打的同样文本不算数。
    ///
    /// 先按 `MessageKind` 筛是必需的：否则用户在对话里贴一段带标记的文本，
    /// 就能让某个 section 永远不再重发。
    #[test]
    fn a_normal_user_message_with_the_same_text_does_not_count() {
        let retained = RetainedSections::scan(&[ConversationItem::real(Message::text(
            Role::User,
            "<available_skills>\n</available_skills>",
        ))]);

        assert_eq!(retained, RetainedSections::default());
    }

    /// 空投影里什么都没有。
    #[test]
    fn an_empty_projection_retains_nothing() {
        assert_eq!(RetainedSections::scan(&[]), RetainedSections::default());
    }
}
