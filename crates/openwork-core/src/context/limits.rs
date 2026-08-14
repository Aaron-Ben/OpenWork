use openwork_models::model::ModelCapabilities;

const DEFAULT_MAX_CONTEXT_ITEM_TOKENS: u32 = 10_000;
const DEFAULT_MAX_USER_INPUT_TOKENS: u32 = 10_000;
const DEFAULT_MAX_TOOL_RESULT_TOKENS: u32 = 8_000;
const DEFAULT_MAX_AGENT_MESSAGE_TOKENS: u32 = 4_000;
const DEFAULT_MAX_SKILL_INSTRUCTION_TOKENS: u32 = 8_000;
/// 摘要写不完就是压缩失败，而压缩发生在窗口快满时——最不能失败的时刻。
/// 这个值沿用产品实际运行过的额度，没有实测数据支持收紧之前不要动它。
const DEFAULT_MAX_COMPACTION_SUMMARY_TOKENS: u32 = 16_384;
pub(crate) const AUTO_COMPACT_THRESHOLD_PERCENT: u8 = 85;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ModelContextLimits {
    pub(crate) context_window_tokens: u64,
    pub(crate) effective_input_tokens: u64,
    pub(crate) accepts_data_blocks: bool,
    pub(crate) max_output_tokens: Option<u32>,
    /// 需要与 `max_output_tokens` 一起从窗口预留的推理额度，见 `ModelCapabilities`。
    pub(crate) max_reasoning_tokens: Option<u32>,
    pub(crate) max_context_item_tokens: u32,
    /// 单条真实用户消息里，一段正文的上限。
    ///
    /// 超限不拒绝也不裁剪，而是转存为工作区文件引用，见 `admission.rs`。
    pub(crate) max_user_input_tokens: u32,
    pub(crate) max_tool_result_tokens: u32,
    pub(crate) max_agent_message_tokens: u32,
    pub(crate) max_skill_instruction_tokens: u32,
    pub(crate) max_compaction_summary_tokens: u32,
    pub(crate) auto_compact_token_limit: u64,
    pub(crate) compaction_compatibility: Option<String>,
}

impl ModelContextLimits {
    /// 由模型能力推导本次会话的上下文治理策略。
    ///
    /// 窗口是输入与输出共享的，因此可用于输入的额度必须先扣掉输出预留；
    /// 自动压缩阈值再按可用输入额度计算，而不是按整个窗口。
    pub(crate) fn from_capabilities(capabilities: ModelCapabilities) -> Self {
        let effective_input_tokens = capabilities
            .context_window_tokens
            .checked_sub(u64::from(capabilities.max_output_tokens))
            .and_then(|remaining| {
                remaining.checked_sub(u64::from(capabilities.max_reasoning_tokens.unwrap_or(0)))
            })
            .expect("validated model capabilities reserve less generation than the context window");
        let auto_compact_token_limit = u64::try_from(
            (u128::from(effective_input_tokens) * u128::from(AUTO_COMPACT_THRESHOLD_PERCENT))
                .div_ceil(100),
        )
        .unwrap_or(u64::MAX);

        Self {
            context_window_tokens: capabilities.context_window_tokens,
            effective_input_tokens,
            accepts_data_blocks: capabilities.accepts_data_blocks,
            max_output_tokens: Some(capabilities.max_output_tokens),
            max_reasoning_tokens: capabilities.max_reasoning_tokens,
            max_context_item_tokens: DEFAULT_MAX_CONTEXT_ITEM_TOKENS,
            max_user_input_tokens: DEFAULT_MAX_USER_INPUT_TOKENS,
            max_tool_result_tokens: DEFAULT_MAX_TOOL_RESULT_TOKENS,
            max_agent_message_tokens: DEFAULT_MAX_AGENT_MESSAGE_TOKENS,
            max_skill_instruction_tokens: DEFAULT_MAX_SKILL_INSTRUCTION_TOKENS,
            max_compaction_summary_tokens: DEFAULT_MAX_COMPACTION_SUMMARY_TOKENS,
            auto_compact_token_limit,
            compaction_compatibility: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn capabilities() -> ModelCapabilities {
        ModelCapabilities {
            context_window_tokens: 200_000,
            max_output_tokens: 32_000,
            max_reasoning_tokens: None,
            accepts_data_blocks: true,
        }
    }

    /// 输入额度必须先扣掉输出预留。
    ///
    /// 窗口是输入与输出共用的：按整个窗口算输入，等于默认模型一个字都不输出，
    /// 结果是请求发出去才被 Provider 拒绝。
    #[test]
    fn the_effective_input_reserves_room_for_the_output() {
        let limits = ModelContextLimits::from_capabilities(capabilities());

        assert_eq!(limits.context_window_tokens, 200_000);
        assert_eq!(limits.effective_input_tokens, 168_000);
        assert_eq!(limits.max_output_tokens, Some(32_000));
        assert_eq!(limits.max_reasoning_tokens, None);
    }

    /// 推理额度单独计费的模型，必须把它一并从窗口里预留掉。
    ///
    /// 例如 Qwen 把最大输出和最大思维链分开公布，两者都占窗口：只按输出预留
    /// 会低估几万 token，直到请求被 Provider 拒绝才暴露。
    #[test]
    fn a_separate_reasoning_budget_is_reserved_on_top_of_the_output() {
        let limits = ModelContextLimits::from_capabilities(ModelCapabilities {
            max_reasoning_tokens: Some(80_000),
            ..capabilities()
        });

        assert_eq!(limits.effective_input_tokens, 88_000);
        assert_eq!(limits.max_reasoning_tokens, Some(80_000));
    }

    /// 自动压缩阈值按可用输入额度算，不按整个窗口。
    #[test]
    fn the_auto_compact_limit_is_a_fraction_of_the_effective_input() {
        let limits = ModelContextLimits::from_capabilities(capabilities());

        assert_eq!(limits.auto_compact_token_limit, 142_800);
        assert!(limits.auto_compact_token_limit < limits.effective_input_tokens);
    }

    /// 模态能力原样来自模型，不由 Core 猜。
    #[test]
    fn the_data_capability_comes_from_the_model() {
        let limits = ModelContextLimits::from_capabilities(ModelCapabilities {
            accepts_data_blocks: false,
            ..capabilities()
        });

        assert!(!limits.accepts_data_blocks);
    }
}
