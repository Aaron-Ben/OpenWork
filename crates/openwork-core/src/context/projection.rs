//! 工具结果的有界模型投影。
//!
//! 工具向 Core 返回的是**权威结果**：它进数据库、进 transcript、进 UI，是业务
//! 事实。模型看到的是它的**有界副本**。两者必须分离——投影只在请求副本里发生，
//! 不回写 `messages.content`，否则 rewind、原文回读和 UI 会失去事实依据。
//!
//! 本模块只投影工具结果。Agent Message 与 Skill 正文的上限是**写入时的准入
//! 检查**（超限拒绝），不是投影，不在这里。

use openwork_chat_state::ConversationItem;
use openwork_models::model::ContentBlock;

use super::budget::estimate_tokens;
use super::ModelContextLimits;

/// 截断后保留在头部的预算占比，其余留给尾部。
///
/// 头部通常是命令、路径和结构，尾部通常是结论和错误——两端都比中间有信息量。
pub(crate) const TOOL_RESULT_HEAD_PERCENT: u64 = 75;

const MARKER_PREFIX: &str = "\n...[tool result truncated;";
const MARKER_SUFFIX: &str = "]...\n";

/// 截断标记。
///
/// 它自身进入模型可见字节，因此必须计入预算，并且对同一对 token 数产生逐字节
/// 一致的结果。
pub(crate) fn truncation_marker(original_tokens: u64, projected_tokens: u64) -> String {
    format!(
        "{MARKER_PREFIX} original≈{original_tokens} tokens; projected={projected_tokens}; \
         continue with read/grep using narrower offset or query{MARKER_SUFFIX}"
    )
}

/// 一次投影改动了什么。供 Trace 与 Inspector 解释请求副本与权威结果的差异。
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ProjectionSummary {
    pub(crate) truncated_tool_results: u32,
    /// 被截断项在投影前的估算 token 总量。
    pub(crate) original_tokens: u64,
    /// 同一批项在投影后的估算 token 总量。
    pub(crate) projected_tokens: u64,
}

#[derive(Debug)]
pub(crate) struct ProjectedConversation {
    pub(crate) items: Vec<ConversationItem>,
    pub(crate) summary: ProjectionSummary,
}

/// 按模型能力生成 Conversation 的有界副本。
///
/// 输入不被改写。同一输入与同一 `limits` 必须得到逐字节一致的输出。
pub(crate) fn project_items(
    items: &[ConversationItem],
    limits: &ModelContextLimits,
) -> ProjectedConversation {
    let mut projected = items.to_vec();
    let mut summary = ProjectionSummary::default();

    for item in &mut projected {
        for block in &mut item.message.content {
            let ContentBlock::ToolResult(result) = block else {
                continue;
            };
            let text_bytes = result.output.iter().fold(0_u64, |bytes, block| {
                let ContentBlock::Text(text) = block else {
                    return bytes;
                };
                bytes.saturating_add(u64::try_from(text.text.len()).unwrap_or(u64::MAX))
            });
            let original_tokens = estimate_tokens(text_bytes);
            let limit = u64::from(limits.max_tool_result_tokens);
            if original_tokens <= limit {
                continue;
            }

            let mut original = String::with_capacity(
                usize::try_from(text_bytes).unwrap_or(usize::MAX),
            );
            for block in &result.output {
                if let ContentBlock::Text(text) = block {
                    original.push_str(&text.text);
                }
            }
            let truncated = truncate_text(&original, original_tokens, limit);
            let projected_tokens = estimate_tokens(
                u64::try_from(truncated.len()).unwrap_or(u64::MAX),
            );
            let mut replacement = Some(truncated);
            for block in &mut result.output {
                if let ContentBlock::Text(text) = block {
                    text.text = replacement.take().unwrap_or_default();
                }
            }
            summary.truncated_tool_results = summary.truncated_tool_results.saturating_add(1);
            summary.original_tokens = summary.original_tokens.saturating_add(original_tokens);
            summary.projected_tokens = summary.projected_tokens.saturating_add(projected_tokens);
        }
    }

    ProjectedConversation {
        items: projected,
        summary,
    }
}

fn truncate_text(text: &str, original_tokens: u64, limit: u64) -> String {
    let max_bytes = usize::try_from(limit.saturating_mul(4)).unwrap_or(usize::MAX);
    let marker = truncation_marker(original_tokens, limit);
    if marker.len() >= max_bytes {
        return marker[..floor_char_boundary(&marker, max_bytes)].to_string();
    }

    let content_bytes = max_bytes - marker.len();
    let head_budget = content_bytes.saturating_mul(
        usize::try_from(TOOL_RESULT_HEAD_PERCENT).unwrap_or(usize::MAX),
    ) / 100;
    let tail_budget = content_bytes - head_budget;
    let head_end = floor_char_boundary(text, head_budget.min(text.len()));
    let tail_start = ceil_char_boundary(text, text.len().saturating_sub(tail_budget));

    format!("{}{}{}", &text[..head_end], marker, &text[tail_start..])
}

fn floor_char_boundary(text: &str, mut index: usize) -> usize {
    while index > 0 && !text.is_char_boundary(index) {
        index -= 1;
    }
    index
}

fn ceil_char_boundary(text: &str, mut index: usize) -> usize {
    while index < text.len() && !text.is_char_boundary(index) {
        index += 1;
    }
    index
}

#[cfg(test)]
mod tests {
    use openwork_models::model::{
        ContentBlock, Message, Role, ToolResultArtifact, ToolResultBlock, ToolResultState,
    };

    use super::*;

    /// 小额度让断言可读；口径与生产一致，只是数值不同。
    fn limits_with_tool_result_tokens(max_tool_result_tokens: u32) -> ModelContextLimits {
        ModelContextLimits {
            max_tool_result_tokens,
            ..ModelContextLimits::for_context_window(200_000)
        }
    }

    fn tool_result_item(text: &str) -> ConversationItem {
        ConversationItem::real(Message {
            role: Role::Tool,
            content: vec![ContentBlock::ToolResult(ToolResultBlock {
                id: "call-1".to_string(),
                name: "read".to_string(),
                output: vec![ContentBlock::text(text)],
                state: ToolResultState::Success,
                artifacts: Vec::new(),
            })],
        })
    }

    fn only_result(item: &ConversationItem) -> &ToolResultBlock {
        match &item.message.content[0] {
            ContentBlock::ToolResult(result) => result,
            other => std::panic::panic_any(format!("expected a tool result, got {other:?}")),
        }
    }

    fn result_text(item: &ConversationItem) -> String {
        only_result(item)
            .output
            .iter()
            .map(|block| match block {
                ContentBlock::Text(text) => text.text.clone(),
                other => std::panic::panic_any(format!("expected text output, got {other:?}")),
            })
            .collect()
    }

    /// 与 `budget.rs` 同一口径：每 4 字节约一个 token，向上取整。
    fn estimated_tokens(text: &str) -> u64 {
        (text.len() as u64).div_ceil(4)
    }

    /// 把投影结果拆成 头 / 尾，marker 本身丢弃。
    fn split_around_marker(projected: &str) -> (String, String) {
        let (head, rest) = projected
            .split_once(MARKER_PREFIX)
            .unwrap_or_else(|| std::panic::panic_any("projected text must carry the marker"));
        let (_, tail) = rest
            .split_once(MARKER_SUFFIX)
            .unwrap_or_else(|| std::panic::panic_any("marker must be closed"));
        (head.to_string(), tail.to_string())
    }

    /// §14.1 #2：工具结果无法超过模型上限。
    #[test]
    fn oversized_tool_result_is_bounded_by_the_limit() {
        let original = "a".repeat(10_000);
        let items = vec![tool_result_item(&original)];

        let projected = project_items(&items, &limits_with_tool_result_tokens(100));

        let text = result_text(&projected.items[0]);
        assert!(
            estimated_tokens(&text) <= 100,
            "投影后 {} tokens 超过上限 100",
            estimated_tokens(&text)
        );
        assert_eq!(projected.summary.truncated_tool_results, 1);
    }

    /// marker 自身计入预算：额度极小时总量仍不得越界。
    #[test]
    fn the_marker_is_counted_inside_the_budget() {
        let original = "b".repeat(10_000);
        let items = vec![tool_result_item(&original)];

        let projected = project_items(&items, &limits_with_tool_result_tokens(60));

        let text = result_text(&projected.items[0]);
        assert!(text.contains("tool result truncated"), "必须带截断标记");
        assert!(
            estimated_tokens(&text) <= 60,
            "含 marker 后 {} tokens 超过上限 60",
            estimated_tokens(&text)
        );
    }

    /// 保留首尾，丢中间。头尾都必须是原文的真实片段。
    #[test]
    fn projection_keeps_a_prefix_and_a_suffix_of_the_original() {
        let original = format!("HEAD_MARK{}TAIL_MARK", "x".repeat(10_000));
        let items = vec![tool_result_item(&original)];

        let projected = project_items(&items, &limits_with_tool_result_tokens(200));

        let (head, tail) = split_around_marker(&result_text(&projected.items[0]));
        assert!(head.starts_with("HEAD_MARK"), "头部必须来自原文开头");
        assert!(tail.ends_with("TAIL_MARK"), "尾部必须来自原文结尾");
        assert!(original.starts_with(&head), "头部必须是原文的真实前缀");
        assert!(original.ends_with(&tail), "尾部必须是原文的真实后缀");
    }

    /// 多字节字符不得被从中间切断。
    #[test]
    fn multi_byte_text_is_never_split_mid_character() {
        let original = "上下文工程改造".repeat(500);
        let items = vec![tool_result_item(&original)];

        let projected = project_items(&items, &limits_with_tool_result_tokens(120));

        let (head, tail) = split_around_marker(&result_text(&projected.items[0]));
        assert!(original.starts_with(&head), "头部必须落在字符边界上");
        assert!(original.ends_with(&tail), "尾部必须落在字符边界上");
    }

    /// 同一输入与同一能力必须得到逐字节一致的投影，否则前缀缓存每轮失效。
    #[test]
    fn projecting_the_same_input_twice_produces_identical_bytes() {
        let items = vec![tool_result_item(&"c".repeat(10_000))];
        let limits = limits_with_tool_result_tokens(100);

        let first = project_items(&items, &limits);
        let second = project_items(&items, &limits);

        assert_eq!(first.items, second.items);
        assert_eq!(first.summary, second.summary);
    }

    /// §14.1 #3：权威结果不被投影污染。
    #[test]
    fn the_authoritative_items_are_left_untouched() {
        let original = "d".repeat(10_000);
        let items = vec![tool_result_item(&original)];
        let before = items.clone();

        let projected = project_items(&items, &limits_with_tool_result_tokens(100));

        assert_eq!(items, before, "投影不得改写输入");
        assert_eq!(result_text(&items[0]), original, "权威结果必须保持全文");
        assert_ne!(
            result_text(&projected.items[0]),
            original,
            "投影必须确实截断了"
        );
    }

    /// 额度内的结果逐字节原样通过，不加 marker。
    #[test]
    fn results_within_budget_pass_through_unchanged() {
        let items = vec![tool_result_item("short output")];

        let projected = project_items(&items, &limits_with_tool_result_tokens(100));

        assert_eq!(projected.items, items);
        assert_eq!(projected.summary, ProjectionSummary::default());
    }

    /// 截断不得改变结果的身份：配对靠 id，UI 靠 name/state/artifacts。
    #[test]
    fn tool_result_identity_survives_projection() {
        let artifact = ToolResultArtifact {
            kind: "file_change".to_string(),
            payload: serde_json::json!({ "changeId": "change-1" }),
        };
        let items = vec![ConversationItem::real(Message {
            role: Role::Tool,
            content: vec![ContentBlock::ToolResult(ToolResultBlock {
                id: "call-7".to_string(),
                name: "bash".to_string(),
                output: vec![ContentBlock::text("e".repeat(10_000))],
                state: ToolResultState::Error,
                artifacts: vec![artifact.clone()],
            })],
        })];

        let projected = project_items(&items, &limits_with_tool_result_tokens(100));

        let result = only_result(&projected.items[0]);
        assert_eq!(result.id, "call-7");
        assert_eq!(result.name, "bash");
        assert_eq!(result.state, ToolResultState::Error);
        assert_eq!(result.artifacts, vec![artifact]);
    }

    /// 本步只投影工具结果。用户与助手正文不在这里截断。
    #[test]
    fn non_tool_messages_are_not_truncated_here() {
        let items = vec![ConversationItem::real(Message::text(
            Role::User,
            "f".repeat(10_000),
        ))];

        let projected = project_items(&items, &limits_with_tool_result_tokens(100));

        assert_eq!(projected.items, items);
        assert_eq!(projected.summary, ProjectionSummary::default());
    }
}
