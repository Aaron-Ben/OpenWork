//! 三个 section 共用的比较与声明逻辑。
//!
//! 它们的差异只有标识和声明文案，状态机是同一套，所以写一处、测一处。将来某个
//! section 真的需要增量渲染时（§8.2），它自己实现 `render_diff` 即可，不影响其余。

use super::{PreviousSectionState, WorldStateFragment};

/// section 正文之外要附加的两句声明。
///
/// 全量重渲染必须自带声明，否则模型无从判断新正文是取代旧的还是补充旧的（§8.2）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct BodyNotices {
    /// 正文变化时前置。例：`以下 AGENTS.md 指令取代先前提供的全部 AGENTS.md 指令。`
    pub(crate) replacement: &'static str,
    /// section 由存在变为缺失时**单独**发送，不能静默消失（§8.1.1）。
    pub(crate) removal: &'static str,
}

/// 比较当前正文与基线，决定这次要不要发、发什么。
///
/// 判定表（`may_contain` = 基线里可能已经有本 section 的正文）：
///
/// | current | may_contain | 结果 |
/// |---|---|---|
/// | `Some` | true | 替换声明 + 正文 |
/// | `Some` | false | 正文（首次出现，无需声明） |
/// | `None` | true | 失效声明 |
/// | `None` | false | 不发 |
///
/// `Known` 且与当前相等时提前返回 `None`，这是"没变就不发"的唯一出口。
pub(crate) fn render_body_diff(
    section_id: &'static str,
    notices: BodyNotices,
    current: Option<&str>,
    previous: PreviousSectionState<'_, Option<String>>,
) -> Option<WorldStateFragment> {
    let may_contain = match previous {
        PreviousSectionState::Known(previous) if previous.as_deref() == current => return None,
        PreviousSectionState::Known(previous) => previous.is_some(),
        PreviousSectionState::Unknown => true,
        PreviousSectionState::Absent => false,
    };

    let body = match (current, may_contain) {
        (Some(current), true) => format!("{}\n\n{current}", notices.replacement),
        (Some(current), false) => current.to_string(),
        (None, true) => notices.removal.to_string(),
        (None, false) => return None,
    };
    Some(WorldStateFragment::text(section_id, body))
}

#[cfg(test)]
mod tests {
    use super::*;

    const ID: &str = "runtime/test-section";
    const NOTICES: BodyNotices = BodyNotices {
        replacement: "取代声明。",
        removal: "失效声明。",
    };

    fn render(
        current: Option<&str>,
        previous: PreviousSectionState<'_, Option<String>>,
    ) -> Option<String> {
        render_body_diff(ID, NOTICES, current, previous).map(|fragment| {
            assert_eq!(fragment.section_id, ID);
            match fragment.content.as_slice() {
                [openwork_models::model::ContentBlock::Text(text)] => text.text.clone(),
                other => panic!("fragment must be exactly one text block, got {other:?}"),
            }
        })
    }

    /// 首次出现不带声明：模型此前没见过，没有要取代的东西。
    #[test]
    fn a_first_appearance_carries_no_notice() {
        assert_eq!(
            render(Some("正文"), PreviousSectionState::Absent),
            Some("正文".to_string())
        );
    }

    /// 正文没变就不发。这是整个改造省 token 的地方。
    #[test]
    fn an_unchanged_body_emits_nothing() {
        let previous = Some("正文".to_string());

        assert_eq!(
            render(Some("正文"), PreviousSectionState::Known(&previous)),
            None
        );
    }

    /// 正文变了：发全量，并前置取代声明。
    ///
    /// 不发文本差异——"把这段改动应用到你记得的那份上"对模型不可靠，指令类内容尤其。
    #[test]
    fn a_changed_body_is_resent_whole_with_the_replacement_notice() {
        let previous = Some("旧正文".to_string());

        assert_eq!(
            render(Some("新正文"), PreviousSectionState::Known(&previous)),
            Some("取代声明。\n\n新正文".to_string())
        );
    }

    /// section 消失必须显式说明，不能静默不发。
    ///
    /// 静默的话模型会继续按已经删掉的 AGENTS.md 行事。
    #[test]
    fn a_disappearing_section_emits_the_removal_notice() {
        let previous = Some("旧正文".to_string());

        assert_eq!(
            render(None, PreviousSectionState::Known(&previous)),
            Some("失效声明。".to_string())
        );
    }

    /// 一直不存在就一直不发，不要为"没有"发一条消息。
    #[test]
    fn a_section_that_was_never_present_stays_silent() {
        let previous = None;

        assert_eq!(render(None, PreviousSectionState::Known(&previous)), None);
        assert_eq!(render(None, PreviousSectionState::Absent), None);
    }

    /// 基线里是"缺失"，现在出现了：算首次出现，不带取代声明。
    #[test]
    fn appearing_after_a_known_absence_carries_no_notice() {
        let previous = None;

        assert_eq!(
            render(Some("正文"), PreviousSectionState::Known(&previous)),
            Some("正文".to_string())
        );
    }

    /// `Unknown` 按"模型可能还记得旧值"处理，带声明重发（§9.4）。
    ///
    /// 当作首次出现是错的：恢复会话后模型上下文里可能仍有上一份正文，不声明取代
    /// 就会同时存在两份互相矛盾的指令。
    #[test]
    fn an_unknown_baseline_is_treated_as_possibly_holding_an_old_body() {
        assert_eq!(
            render(Some("正文"), PreviousSectionState::Unknown),
            Some("取代声明。\n\n正文".to_string())
        );
    }

    /// `Unknown` 且当前已缺失：同样要发失效声明。
    #[test]
    fn an_unknown_baseline_with_no_body_emits_the_removal_notice() {
        assert_eq!(
            render(None, PreviousSectionState::Unknown),
            Some("失效声明。".to_string())
        );
    }
}
