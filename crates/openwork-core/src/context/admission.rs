//! 真实用户输入的准入检查。
//!
//! 上下文里的其他一切超限时都可以裁剪或拒绝，唯独用户自己写下的内容不行：
//! 静默裁掉中间段会让模型回答一个与用户看到的输入不同的问题，直接拒绝 Turn
//! 又会把一次正常的粘贴变成一堵墙。
//!
//! 因此超限正文被**转存**到工作区文件，正文位置换成一条带路径和原始大小的
//! 引用。用户在自己的消息里就能看到这次转存，模型可以用 `read` 按需分段查看。
//!
//! 本模块只做**决策**，不碰文件系统：它产出"该写哪些文件、写什么"以及转存
//! 生效后的内容。真正的写盘留在调用侧，好让这里的规则可以被纯函数测试覆盖。

use openwork_models::model::ContentBlock;

use super::ModelContextLimits;
use super::budget::estimate_tokens;

/// 转存文件所在的工作区相对目录。
pub(crate) const SPILL_DIR: &str = ".openwork/pasted";

/// 转存后留在正文里的引用。
///
/// 它进入模型可见字节，必须对同一对参数产生逐字节一致的结果。
pub(crate) fn spill_reference_text(relative_path: &str, original_bytes: u64) -> String {
    format!(
        "[粘贴内容已转存：{relative_path}（{original_bytes} 字节）。\
         需要时用 read 按 offset/limit 分段查看，不要凭这条引用猜测其内容。]"
    )
}

/// 一份待写出的转存文件。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PlannedSpill {
    /// 工作区相对路径，例如 `.openwork/pasted/req-7-1.txt`。
    pub(crate) relative_path: String,
    /// 待写出的完整原文，一个字节都不能少。
    pub(crate) text: String,
    pub(crate) original_bytes: u64,
}

/// 准入决策的结果。
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct UserInputAdmission {
    /// 转存生效后的用户消息内容。
    pub(crate) content: Vec<ContentBlock>,
    /// 需要写出的文件，按正文出现顺序排列。空表示没有任何内容超限。
    pub(crate) spills: Vec<PlannedSpill>,
}

/// 决定一条真实用户消息里哪些正文需要转存。
///
/// `request_key` 必须在同一次提交的重试之间保持稳定（用 client request id），
/// 否则重试会写出重复文件并改变模型可见字节。
pub(crate) fn plan_user_input_admission(
    content: &[ContentBlock],
    limits: &ModelContextLimits,
    request_key: &str,
) -> UserInputAdmission {
    let mut spills = Vec::new();
    let content = content
        .iter()
        .map(|block| {
            let ContentBlock::Text(text) = block else {
                return block.clone();
            };
            let original_bytes = u64::try_from(text.text.len()).unwrap_or(u64::MAX);
            if estimate_tokens(original_bytes) <= u64::from(limits.max_user_input_tokens) {
                return block.clone();
            }

            let ordinal = spills.len().saturating_add(1);
            let relative_path = format!("{SPILL_DIR}/{request_key}-{ordinal}.txt");
            spills.push(PlannedSpill {
                relative_path: relative_path.clone(),
                text: text.text.clone(),
                original_bytes,
            });
            ContentBlock::text(spill_reference_text(&relative_path, original_bytes))
        })
        .collect();

    UserInputAdmission { content, spills }
}

#[cfg(test)]
mod tests {
    use openwork_models::model::ModelCapabilities;

    use super::*;

    fn limits_with_user_input_tokens(max_user_input_tokens: u32) -> ModelContextLimits {
        ModelContextLimits {
            max_user_input_tokens,
            ..ModelContextLimits::from_capabilities(ModelCapabilities {
                context_window_tokens: 200_000,
                max_output_tokens: 32_000,
                max_reasoning_tokens: None,
                accepts_data_blocks: true,
            })
        }
    }

    /// 与 `budget.rs` 同一口径：每 4 字节约一个 token，向上取整。
    fn estimated_tokens(text: &str) -> u64 {
        (text.len() as u64).div_ceil(4)
    }

    fn text_of(block: &ContentBlock) -> &str {
        match block {
            ContentBlock::Text(text) => text.text.as_str(),
            other => std::panic::panic_any(format!("expected text, got {other:?}")),
        }
    }

    /// 额度内的输入逐字节原样通过，不产生任何文件。
    #[test]
    fn input_within_the_limit_passes_through_unchanged() {
        let content = vec![ContentBlock::text("帮我看下 run_loop 里的压缩触发")];

        let admission =
            plan_user_input_admission(&content, &limits_with_user_input_tokens(100), "req-1");

        assert_eq!(admission.content, content);
        assert!(admission.spills.is_empty());
    }

    /// §14.1 #14：超限正文转存为文件引用，Turn 不被拒绝，原文一字不少。
    #[test]
    fn oversized_text_becomes_a_reference_and_the_original_is_planned_for_spill() {
        let original = "日志".repeat(5_000);
        let content = vec![ContentBlock::text(&original)];

        let admission =
            plan_user_input_admission(&content, &limits_with_user_input_tokens(100), "req-7");

        assert_eq!(admission.spills.len(), 1);
        let spill = &admission.spills[0];
        assert_eq!(spill.text, original, "转存文件必须是完整原文");
        assert_eq!(spill.original_bytes, original.len() as u64);
        assert_eq!(spill.relative_path, ".openwork/pasted/req-7-1.txt");

        assert_eq!(admission.content.len(), 1);
        assert_eq!(
            text_of(&admission.content[0]),
            spill_reference_text(&spill.relative_path, spill.original_bytes)
        );
    }

    /// 引用本身必须落在额度内，否则转存等于没做。
    #[test]
    fn the_reference_fits_inside_the_limit() {
        let content = vec![ContentBlock::text("x".repeat(80_000))];

        let admission =
            plan_user_input_admission(&content, &limits_with_user_input_tokens(100), "req-2");

        assert!(
            estimated_tokens(text_of(&admission.content[0])) <= 100,
            "引用文本自身超出了额度"
        );
    }

    /// 只转存超限的块，其余原样保留，顺序不变。
    #[test]
    fn only_oversized_blocks_are_spilled_and_order_is_preserved() {
        let long = "y".repeat(80_000);
        let content = vec![
            ContentBlock::text("先看这段日志："),
            ContentBlock::text(&long),
            ContentBlock::text("有什么问题？"),
        ];

        let admission =
            plan_user_input_admission(&content, &limits_with_user_input_tokens(100), "req-3");

        assert_eq!(admission.spills.len(), 1);
        assert_eq!(text_of(&admission.content[0]), "先看这段日志：");
        assert_eq!(
            text_of(&admission.content[1]),
            spill_reference_text(".openwork/pasted/req-3-1.txt", long.len() as u64)
        );
        assert_eq!(text_of(&admission.content[2]), "有什么问题？");
    }

    /// 多个超限块按出现顺序编号，从 1 开始。
    #[test]
    fn multiple_spills_are_numbered_in_order() {
        let content = vec![
            ContentBlock::text("a".repeat(80_000)),
            ContentBlock::text("短的"),
            ContentBlock::text("b".repeat(80_000)),
        ];

        let admission =
            plan_user_input_admission(&content, &limits_with_user_input_tokens(100), "req-4");

        assert_eq!(
            admission
                .spills
                .iter()
                .map(|spill| spill.relative_path.as_str())
                .collect::<Vec<_>>(),
            [
                ".openwork/pasted/req-4-1.txt",
                ".openwork/pasted/req-4-2.txt"
            ]
        );
    }

    /// 同一次提交重试时必须得到同一批路径与同样的模型可见字节。
    #[test]
    fn planning_is_deterministic_for_the_same_request_key() {
        let content = vec![ContentBlock::text("z".repeat(80_000))];
        let limits = limits_with_user_input_tokens(100);

        let first = plan_user_input_admission(&content, &limits, "req-5");
        let second = plan_user_input_admission(&content, &limits, "req-5");

        assert_eq!(first, second);
    }

    /// 决策不碰非文本内容：图片的上限属于模态与投影，不属于本模块。
    #[test]
    fn non_text_blocks_are_left_to_other_stages() {
        let content = vec![ContentBlock::image_url(
            "https://example.test/a.png",
            "image/png",
        )];

        let admission =
            plan_user_input_admission(&content, &limits_with_user_input_tokens(100), "req-6");

        assert_eq!(admission.content, content);
        assert!(admission.spills.is_empty());
    }
}
