const DEFAULT_MAX_CONTEXT_ITEM_TOKENS: u32 = 10_000;
const DEFAULT_MAX_TOOL_RESULT_TOKENS: u32 = 8_000;
const DEFAULT_MAX_AGENT_MESSAGE_TOKENS: u32 = 4_000;
const DEFAULT_MAX_SKILL_INSTRUCTION_TOKENS: u32 = 8_000;
const DEFAULT_MAX_COMPACTION_SUMMARY_TOKENS: u32 = 8_000;
const DEFAULT_AUTO_COMPACTION_THRESHOLD_PERCENT: u64 = 85;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ModelContextLimits {
    pub(crate) context_window_tokens: u64,
    pub(crate) effective_input_tokens: u64,
    pub(crate) max_output_tokens: Option<u32>,
    pub(crate) reasoning_headroom_tokens: Option<u32>,
    pub(crate) max_context_item_tokens: u32,
    pub(crate) max_tool_result_tokens: u32,
    pub(crate) max_agent_message_tokens: u32,
    pub(crate) max_skill_instruction_tokens: u32,
    pub(crate) max_compaction_summary_tokens: u32,
    pub(crate) auto_compact_token_limit: u64,
    pub(crate) compaction_compatibility: Option<String>,
}

impl ModelContextLimits {
    pub(crate) fn for_context_window(context_window_tokens: u64) -> Self {
        let effective_input_tokens = context_window_tokens;
        let auto_compact_token_limit = u64::try_from(
            (u128::from(effective_input_tokens)
                * u128::from(DEFAULT_AUTO_COMPACTION_THRESHOLD_PERCENT))
            .div_ceil(100),
        )
        .unwrap_or(u64::MAX);

        Self {
            context_window_tokens,
            effective_input_tokens,
            max_output_tokens: None,
            reasoning_headroom_tokens: None,
            max_context_item_tokens: DEFAULT_MAX_CONTEXT_ITEM_TOKENS,
            max_tool_result_tokens: DEFAULT_MAX_TOOL_RESULT_TOKENS,
            max_agent_message_tokens: DEFAULT_MAX_AGENT_MESSAGE_TOKENS,
            max_skill_instruction_tokens: DEFAULT_MAX_SKILL_INSTRUCTION_TOKENS,
            max_compaction_summary_tokens: DEFAULT_MAX_COMPACTION_SUMMARY_TOKENS,
            auto_compact_token_limit,
            compaction_compatibility: None,
        }
    }
}
