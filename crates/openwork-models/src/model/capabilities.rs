//! 模型能力：一次 Model Call 的额度事实，不含任何治理策略。
//!
//! 这里只回答"这个模型能吃多少、我们给它留多少输出、收不收图片"。用多少算满、
//! 单项裁到多大、什么时候压缩，都是 Core 的策略，属于 `openwork-core` 的
//! `ModelContextLimits`，不属于这里。
//!
//! 这些值必须由 Provider preset 或用户显式配置提供。未知模型不能靠推断——
//! 猜错窗口的后果是请求被 Provider 拒绝或历史被无谓压缩，两者都不会有明确报错。

use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelCapabilities {
    /// 输入与全部生成内容共享的总窗口。这是厂商事实。
    pub context_window_tokens: u64,
    /// 本产品为这个模型设定的输出额度，直接写进请求的 `max_output_tokens`。
    ///
    /// **这是我们的选择，不是厂商能力。** 厂商给出固定上限时不得超过它；
    /// 厂商把输出定义为"窗口减去 prompt"（例如 Kimi）时，这个值完全由我们决定。
    pub max_output_tokens: u32,
    /// 厂商单独公布的推理 token 额度，仅当它**不计入** `max_output_tokens`
    /// 却仍然占用窗口时才有值（例如 Qwen 的思维链额度）。
    ///
    /// 有值时它与输出额度相加才是需要从窗口里预留的总量。绝大多数模型为
    /// `None`：推理 token 本来就算在输出额度里。
    pub max_reasoning_tokens: Option<u32>,
    /// 是否接受 `ContentBlock::Data`。这是厂商事实。
    pub accepts_data_blocks: bool,
}

impl ModelCapabilities {
    pub fn validate(self) -> Result<Self, ModelCapabilitiesError> {
        if self.context_window_tokens == 0 {
            return Err(ModelCapabilitiesError::EmptyContextWindow);
        }
        if self.max_output_tokens == 0 {
            return Err(ModelCapabilitiesError::EmptyOutputLimit);
        }
        let reserved_generation_tokens = u64::from(self.max_output_tokens)
            .checked_add(u64::from(self.max_reasoning_tokens.unwrap_or(0)))
            .ok_or(ModelCapabilitiesError::GenerationReservationOverflow)?;
        if reserved_generation_tokens >= self.context_window_tokens {
            return Err(
                ModelCapabilitiesError::GenerationReservationExhaustsWindow {
                    reserved_generation_tokens,
                    context_window_tokens: self.context_window_tokens,
                },
            );
        }
        Ok(self)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ModelCapabilitiesError {
    #[error("contextWindowTokens must be positive")]
    EmptyContextWindow,
    #[error("maxOutputTokens must be positive")]
    EmptyOutputLimit,
    #[error("the output and reasoning reservation overflowed")]
    GenerationReservationOverflow,
    #[error(
        "output and reasoning reserve {reserved_generation_tokens} tokens, which must be less than the {context_window_tokens}-token context window"
    )]
    GenerationReservationExhaustsWindow {
        reserved_generation_tokens: u64,
        context_window_tokens: u64,
    },
}
