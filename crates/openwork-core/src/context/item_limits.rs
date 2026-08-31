//! 模型可见单项的硬上限。
//!
//! 工具结果和用户输入有各自的处置方式（截断、转存），在 `projection.rs` 与
//! `admission.rs`。这里管剩下几类：它们的正文都由作者控制——仓库里的
//! `AGENTS.md`、Skill 的 `SKILL.md`、子智能体自己写的汇报——所以超限时**拒绝
//! 并说清怎么改**，而不是裁剪。裁掉一半的指令比没有指令更糟：模型会照着半条
//! 规则行事，而没人知道另一半去哪了。

use thiserror::Error;

use super::ModelContextLimits;

/// 一个受上限约束的模型可见单项。
///
/// 每一类用各自的上限，不共用一个数：System 片段是常驻成本，Skill 正文是
/// 一次性注入，子智能体汇报是运行中产生的，三者能容忍的体量本就不同。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BoundedItem<'a> {
    /// System Context 的一个片段，`key` 是它的稳定标识。
    SystemContextPart { key: &'a str },
    /// Turn 接受时物化的 Skill 正文。
    SkillInstruction { name: &'a str },
    /// 子智能体发回父 Session 的消息。
    AgentMessage { task_name: &'a str },
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub(crate) enum ItemLimitError {
    #[error(
        "system context part {key} needs {tokens} tokens, over the {limit}-token cap; split the source file or shorten it"
    )]
    SystemContextPart {
        key: String,
        tokens: u64,
        limit: u32,
    },
    #[error(
        "skill {name} needs {tokens} tokens, over the {limit}-token cap; split it into smaller skills"
    )]
    SkillInstruction {
        name: String,
        tokens: u64,
        limit: u32,
    },
    #[error(
        "the message from {task_name} needs {tokens} tokens, over the {limit}-token cap; have the sub-agent report a summary instead"
    )]
    AgentMessage {
        task_name: String,
        tokens: u64,
        limit: u32,
    },
}

/// 校验一个模型可见单项是否在它那一类的上限内。
///
/// `tokens` 必须由 `budget` 的同一口径估算，不要各处自算。
pub(crate) fn check_item_tokens(
    item: BoundedItem<'_>,
    tokens: u64,
    limits: &ModelContextLimits,
) -> Result<(), ItemLimitError> {
    let limit = match item {
        BoundedItem::SystemContextPart { .. } => limits.max_context_item_tokens,
        BoundedItem::SkillInstruction { .. } => limits.max_skill_instruction_tokens,
        BoundedItem::AgentMessage { .. } => limits.max_agent_message_tokens,
    };
    if tokens < u64::from(limit) {
        return Ok(());
    }

    Err(match item {
        BoundedItem::SystemContextPart { key } => ItemLimitError::SystemContextPart {
            key: key.to_string(),
            tokens,
            limit,
        },
        BoundedItem::SkillInstruction { name } => ItemLimitError::SkillInstruction {
            name: name.to_string(),
            tokens,
            limit,
        },
        BoundedItem::AgentMessage { task_name } => ItemLimitError::AgentMessage {
            task_name: task_name.to_string(),
            tokens,
            limit,
        },
    })
}

#[cfg(test)]
mod tests {
    use openwork_models::model::ModelCapabilities;

    use super::*;

    fn limits() -> ModelContextLimits {
        ModelContextLimits::from_capabilities(ModelCapabilities {
            context_window_tokens: 200_000,
            max_output_tokens: 32_000,
            max_reasoning_tokens: None,
            accepts_data_blocks: true,
        })
    }

    /// 额度内的单项一律放行。
    #[test]
    fn items_within_their_own_limit_are_accepted() {
        let limits = limits();

        assert_eq!(
            check_item_tokens(
                BoundedItem::SystemContextPart {
                    key: "project/AGENTS.md"
                },
                limits.max_context_item_tokens as u64 - 1,
                &limits
            ),
            Ok(())
        );
        assert_eq!(
            check_item_tokens(
                BoundedItem::SkillInstruction { name: "commit" },
                limits.max_skill_instruction_tokens as u64 - 1,
                &limits
            ),
            Ok(())
        );
        assert_eq!(
            check_item_tokens(
                BoundedItem::AgentMessage {
                    task_name: "explore"
                },
                limits.max_agent_message_tokens as u64 - 1,
                &limits
            ),
            Ok(())
        );
    }

    /// 上限是闭区间外沿：恰好等于上限算超。
    ///
    /// 边界写死一个方向，免得两处实现各理解一半。
    #[test]
    fn an_item_exactly_at_the_limit_is_rejected() {
        let limits = limits();

        assert!(
            check_item_tokens(
                BoundedItem::SkillInstruction { name: "commit" },
                limits.max_skill_instruction_tokens as u64,
                &limits
            )
            .is_err()
        );
    }

    /// System 片段超限时报出是哪一个 key，否则用户无从下手。
    #[test]
    fn an_oversized_system_context_part_names_its_key() {
        let limits = limits();
        let tokens = u64::from(limits.max_context_item_tokens) + 1;

        let error = check_item_tokens(
            BoundedItem::SystemContextPart {
                key: "project/AGENTS.md",
            },
            tokens,
            &limits,
        )
        .expect_err("must reject");

        assert_eq!(
            error,
            ItemLimitError::SystemContextPart {
                key: "project/AGENTS.md".to_string(),
                tokens,
                limit: limits.max_context_item_tokens,
            }
        );
        assert!(error.to_string().contains("project/AGENTS.md"));
    }

    /// Skill 超限时报出是哪个 Skill，并提示拆分。
    #[test]
    fn an_oversized_skill_names_itself_and_suggests_splitting() {
        let limits = limits();
        let tokens = u64::from(limits.max_skill_instruction_tokens) + 1;

        let error = check_item_tokens(
            BoundedItem::SkillInstruction { name: "release" },
            tokens,
            &limits,
        )
        .expect_err("must reject");

        assert_eq!(
            error,
            ItemLimitError::SkillInstruction {
                name: "release".to_string(),
                tokens,
                limit: limits.max_skill_instruction_tokens,
            }
        );
        assert!(error.to_string().contains("split"));
    }

    /// 子智能体消息超限时报出是哪个任务。
    #[test]
    fn an_oversized_agent_message_names_its_task() {
        let limits = limits();
        let tokens = u64::from(limits.max_agent_message_tokens) + 1;

        let error = check_item_tokens(
            BoundedItem::AgentMessage {
                task_name: "explore-storage",
            },
            tokens,
            &limits,
        )
        .expect_err("must reject");

        assert_eq!(
            error,
            ItemLimitError::AgentMessage {
                task_name: "explore-storage".to_string(),
                tokens,
                limit: limits.max_agent_message_tokens,
            }
        );
    }

    /// 每一类用各自的上限，不许合并成一个数。
    ///
    /// 4K 的子智能体消息超限，同样大小的 Skill 正文（8K 上限）不超限。
    #[test]
    fn each_kind_is_measured_against_its_own_limit() {
        let limits = limits();
        let tokens = u64::from(limits.max_agent_message_tokens) + 1;
        assert!(
            tokens < u64::from(limits.max_skill_instruction_tokens),
            "前提：子智能体消息的上限严格小于 Skill 正文的上限"
        );

        assert!(
            check_item_tokens(
                BoundedItem::AgentMessage {
                    task_name: "explore"
                },
                tokens,
                &limits
            )
            .is_err()
        );
        assert_eq!(
            check_item_tokens(
                BoundedItem::SkillInstruction { name: "commit" },
                tokens,
                &limits
            ),
            Ok(())
        );
    }
}
