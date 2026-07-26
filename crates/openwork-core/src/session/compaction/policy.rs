pub const DEFAULT_CONTEXT_WINDOW_TOKENS: u64 = 258_000;
pub const DEFAULT_AUTO_COMPACTION_THRESHOLD_PERCENT: u8 = 85;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct AutomaticCompactionPolicy {
    pub(crate) context_window_tokens: u64,
    pub(crate) threshold_percent: u8,
}

impl AutomaticCompactionPolicy {
    pub(crate) fn new(context_window_tokens: u64, threshold_percent: u8) -> Option<Self> {
        (context_window_tokens > 0 && (1..=100).contains(&threshold_percent)).then_some(Self {
            context_window_tokens,
            threshold_percent,
        })
    }

    pub(crate) fn for_context_window(context_window_tokens: u64) -> Option<Self> {
        Self::new(
            context_window_tokens,
            DEFAULT_AUTO_COMPACTION_THRESHOLD_PERCENT,
        )
    }

    pub(crate) fn should_compact(self, estimated_input_tokens: u64) -> bool {
        u128::from(estimated_input_tokens) * 100
            >= u128::from(self.context_window_tokens) * u128::from(self.threshold_percent)
    }
}

impl Default for AutomaticCompactionPolicy {
    fn default() -> Self {
        Self {
            context_window_tokens: DEFAULT_CONTEXT_WINDOW_TOKENS,
            threshold_percent: DEFAULT_AUTO_COMPACTION_THRESHOLD_PERCENT,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_policy_uses_eighty_five_percent_of_the_default_window() {
        let policy = AutomaticCompactionPolicy::default();

        assert_eq!(policy.context_window_tokens, 258_000);
        assert_eq!(policy.threshold_percent, 85);
        assert!(!policy.should_compact(219_299));
        assert!(policy.should_compact(219_300));
    }

    #[test]
    fn threshold_is_inclusive_and_uses_overflow_safe_arithmetic() {
        let policy = AutomaticCompactionPolicy::new(100, 85).expect("valid policy");

        assert!(!policy.should_compact(84));
        assert!(policy.should_compact(85));
        assert!(policy.should_compact(u64::MAX));
    }

    #[test]
    fn invalid_policy_values_are_rejected() {
        assert!(AutomaticCompactionPolicy::new(0, 85).is_none());
        assert!(AutomaticCompactionPolicy::new(100, 0).is_none());
        assert!(AutomaticCompactionPolicy::new(100, 101).is_none());
    }
}
