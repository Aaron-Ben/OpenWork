//! 请求副本的合法化。
//!
//! 这里产生的是**请求副本**，不是第二份权威 Conversation：输入是 typed
//! [`ConversationItem`]，输出是可以直接交给 Provider 的 [`Message`] 序列。
//! 数据库里的原始消息不因这里的任何决定而改变。
//!
//! 输入保持 typed 形态的原因是投影决策依赖 [`MessageKind`]：真实用户输入与
//! Skill 正文、子智能体消息在 `Role::User` 上无法区分，只有 `MessageKind`
//! 能区分。任何“先摊平成 Message 再处理”的写法都会丢掉这个信息。

use std::collections::HashMap;

use openwork_chat_state::{ConversationItem, MessageKind};
use openwork_models::model::{ContentBlock, Message, Role, ToolResultBlock, ToolResultState};
use thiserror::Error;

/// 合成 Tool Result 的正文。
///
/// 它进入模型可见字节，因此必须是常量：同一段历史每次合法化都要得到逐字节
/// 一致的请求副本，否则前缀缓存每轮都会失效。
pub(crate) const INTERRUPTED_RESULT_TEXT: &str =
    "Tool result unavailable: the previous turn ended before a durable result was recorded.";

/// 模型能力中影响请求副本合法性的部分。
///
/// 这是 [`super::ModelContextLimits`] 的投影，不直接依赖它，好让合法化可以在
/// 不构造完整模型能力的情况下被测试。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct NormalizationPolicy {
    /// 模型是否接受 `ContentBlock::Data`。
    pub(crate) accepts_data_blocks: bool,
}

/// 一次合法化改动了什么。
///
/// 供 Inspector 与 Trace 解释请求副本与权威历史的差异，不参与任何控制流。
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) struct NormalizationReport {
    /// 为缺失结果的 Tool Call 合成的 Interrupted 结果条数。
    pub(crate) synthesized_tool_results: u32,
    /// 匹配不到任何 Tool Call 而被移出请求副本的孤儿结果条数。
    pub(crate) dropped_orphan_results: u32,
    /// 因模型不接受而被移出请求副本的 Data block 数量。
    pub(crate) filtered_data_blocks: u32,
}

#[derive(Debug)]
pub(crate) struct NormalizedConversation {
    pub(crate) messages: Vec<Message>,
    pub(crate) report: NormalizationReport,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub(crate) enum NormalizationError {
    /// 真实用户输入含有模型不接受的 Data block。
    ///
    /// 这一条不静默过滤：用户明确附上的内容被悄悄丢掉，会让模型回答一个
    /// 与用户看到的输入不同的问题。契约性内容（Skill 正文、Agent Message）
    /// 则按能力过滤，不报错。
    #[error("model does not accept data blocks in real user input (item {item_index})")]
    UnsupportedUserData { item_index: usize },
}

/// 把 typed Conversation 合法化成一份可提交的请求副本。
///
/// 顺序见 `docs/research/codex-context-engineering-refactor.md` §7.2 的第 3–5 步。
pub(crate) fn normalize_for_request(
    items: &[ConversationItem],
    policy: &NormalizationPolicy,
) -> Result<NormalizedConversation, NormalizationError> {
    let mut messages = Vec::with_capacity(items.len());
    let mut report = NormalizationReport::default();
    let mut index = 0;
    while index < items.len() {
        let item = &items[index];
        let message = &item.message;
        if message.role != Role::Assistant {
            if message.role == Role::Tool {
                let count = message.content.iter()
                    .filter(|block| matches!(block, ContentBlock::ToolResult(_))).count();
                report.dropped_orphan_results += u32::try_from(count).unwrap_or(u32::MAX);
            } else {
                messages.push(filter_item(item, index, policy, &mut report)?);
            }
            index += 1;
            continue;
        }
        let expected = message
            .content
            .iter()
            .filter_map(|block| match block {
                ContentBlock::ToolCall(call) => Some((call.id.clone(), call.name.clone())),
                _ => None,
            })
            .collect::<Vec<_>>();
        messages.push(filter_item(item, index, policy, &mut report)?);
        if expected.is_empty() {
            index += 1;
            continue;
        }
        index += 1;
        let mut answered = HashMap::with_capacity(expected.len());
        while index < items.len() && items[index].message.role == Role::Tool {
            for block in &items[index].message.content {
                let ContentBlock::ToolResult(result) = block else {
                    continue;
                };
                let matches_call = expected.iter().any(|(id, _)| id == &result.id);
                if matches_call && !answered.contains_key(&result.id) {
                    answered.insert(result.id.clone(), result.clone());
                } else {
                    report.dropped_orphan_results += 1;
                }
            }
            index += 1;
        }
        for (id, name) in expected {
            let mut result = answered.remove(&id).unwrap_or_else(|| {
                report.synthesized_tool_results += 1;
                ToolResultBlock {
                    id,
                    name: name.clone(),
                    output: vec![ContentBlock::text(INTERRUPTED_RESULT_TEXT)],
                    state: ToolResultState::Interrupted,
                    artifacts: Vec::new(),
                }
            });
            result.name = name;
            if !policy.accepts_data_blocks {
                result.output = filter_content_blocks(result.output, false, index, &mut report)?;
            }
            messages.push(Message {
                role: Role::Tool,
                content: vec![ContentBlock::ToolResult(result)],
            });
        }
    }
    Ok(NormalizedConversation { messages, report })
}
fn filter_item(item: &ConversationItem, item_index: usize, policy: &NormalizationPolicy,
    report: &mut NormalizationReport,
) -> Result<Message, NormalizationError> {
    let mut message = item.message.clone();
    if policy.accepts_data_blocks {
        return Ok(message);
    }
    message.content = filter_content_blocks(message.content,
        item.kind == MessageKind::Normal && message.role == Role::User, item_index, report)?;
    Ok(message)
}
fn filter_content_blocks(blocks: Vec<ContentBlock>, reject_data: bool, item_index: usize,
    report: &mut NormalizationReport,
) -> Result<Vec<ContentBlock>, NormalizationError> {
    let mut filtered = Vec::with_capacity(blocks.len());
    for block in blocks {
        match block {
            ContentBlock::Data(_) if reject_data =>
                return Err(NormalizationError::UnsupportedUserData { item_index }),
            ContentBlock::Data(_) => report.filtered_data_blocks += 1,
            ContentBlock::ToolResult(mut result) => {
                result.output = filter_content_blocks(result.output, reject_data, item_index, report)?;
                filtered.push(ContentBlock::ToolResult(result));
            }
            other => filtered.push(other),
        }
    }
    Ok(filtered)
}
#[cfg(test)]
mod tests {
    use openwork_models::model::{
        ContentBlock, Role, ToolCallBlock, ToolCallState, ToolResultBlock, ToolResultState,
    };

    use super::*;

    fn strict_policy() -> NormalizationPolicy {
        NormalizationPolicy {
            accepts_data_blocks: false,
        }
    }

    fn permissive_policy() -> NormalizationPolicy {
        NormalizationPolicy {
            accepts_data_blocks: true,
        }
    }

    fn assistant_calls(calls: &[(&str, &str)]) -> ConversationItem {
        ConversationItem::real(Message {
            role: Role::Assistant,
            content: calls
                .iter()
                .map(|(id, name)| {
                    ContentBlock::ToolCall(ToolCallBlock {
                        id: (*id).to_string(),
                        name: (*name).to_string(),
                        input: "{}".to_string(),
                        state: ToolCallState::Submitted,
                    })
                })
                .collect(),
        })
    }

    fn tool_result(id: &str, name: &str, output: Vec<ContentBlock>) -> ConversationItem {
        ConversationItem::real(Message {
            role: Role::Tool,
            content: vec![ContentBlock::ToolResult(ToolResultBlock {
                id: id.to_string(),
                name: name.to_string(),
                output,
                state: ToolResultState::Success,
                artifacts: Vec::new(),
            })],
        })
    }

    fn only_result(message: &Message) -> &ToolResultBlock {
        match &message.content[0] {
            ContentBlock::ToolResult(result) => result,
            other => std::panic::panic_any(format!("expected a tool result, got {other:?}")),
        }
    }

    /// §14.1 #4：缺失的 Tool Result 只在请求副本里补齐，ID 由原 Call ID 确定性派生。
    #[test]
    fn missing_tool_result_is_synthesized_in_call_order_with_the_original_call_id() {
        let items = vec![
            assistant_calls(&[("call-1", "read"), ("call-2", "list")]),
            tool_result("call-1", "read", vec![ContentBlock::text("file body")]),
        ];

        let normalized =
            normalize_for_request(&items, &strict_policy()).expect("normalization succeeds");

        assert_eq!(
            normalized
                .messages
                .iter()
                .map(|message| message.role)
                .collect::<Vec<_>>(),
            [Role::Assistant, Role::Tool, Role::Tool]
        );
        assert_eq!(only_result(&normalized.messages[1]).id, "call-1");

        let synthesized = only_result(&normalized.messages[2]);
        assert_eq!(synthesized.id, "call-2");
        assert_eq!(synthesized.name, "list");
        assert_eq!(synthesized.state, ToolResultState::Interrupted);
        assert_eq!(
            synthesized.output,
            vec![ContentBlock::text(INTERRUPTED_RESULT_TEXT)]
        );
        assert_eq!(normalized.report.synthesized_tool_results, 1);
    }

    /// 同一段历史反复合法化必须逐字节一致，否则前缀缓存每轮失效。
    #[test]
    fn synthesizing_the_same_history_twice_produces_identical_bytes() {
        let items = vec![assistant_calls(&[("call-1", "read")])];

        let first = normalize_for_request(&items, &strict_policy()).expect("first");
        let second = normalize_for_request(&items, &strict_policy()).expect("second");

        assert_eq!(first.messages, second.messages);
    }

    /// 结果必须紧跟声明它们的 Assistant，并按 Assistant 的 Tool Call 顺序排列。
    ///
    /// 被别的消息挤开的结果不再属于那段连续区间，按孤儿处理，对应的 Call
    /// 补一条 Interrupted。这是 Provider 对工具序列的硬性要求。
    #[test]
    fn results_are_reordered_into_call_order_and_displaced_results_are_dropped() {
        let items = vec![
            assistant_calls(&[("call-1", "read"), ("call-2", "list")]),
            tool_result("call-2", "list", vec![ContentBlock::text("files")]),
            ConversationItem::real(Message::text(Role::User, "continue after failure")),
            tool_result("call-1", "read", vec![ContentBlock::text("late")]),
        ];

        let normalized =
            normalize_for_request(&items, &strict_policy()).expect("normalization succeeds");

        assert_eq!(
            normalized
                .messages
                .iter()
                .map(|message| message.role)
                .collect::<Vec<_>>(),
            [Role::Assistant, Role::Tool, Role::Tool, Role::User]
        );

        let first = only_result(&normalized.messages[1]);
        assert_eq!(first.id, "call-1");
        assert_eq!(first.state, ToolResultState::Interrupted);

        let second = only_result(&normalized.messages[2]);
        assert_eq!(second.id, "call-2");
        assert_eq!(second.state, ToolResultState::Success);

        assert_eq!(normalized.report.synthesized_tool_results, 1);
        assert_eq!(normalized.report.dropped_orphan_results, 1);
    }

    /// §14.1 #5：孤儿结果不进请求副本，输入本身不被改写。
    #[test]
    fn orphan_tool_result_is_dropped_from_the_request_copy() {
        let items = vec![
            ConversationItem::real(Message::text(Role::User, "查一下")),
            tool_result("ghost-call", "read", vec![ContentBlock::text("stale")]),
        ];
        let before = items.clone();

        let normalized =
            normalize_for_request(&items, &strict_policy()).expect("normalization succeeds");

        assert_eq!(normalized.messages, vec![Message::text(Role::User, "查一下")]);
        assert_eq!(normalized.report.dropped_orphan_results, 1);
        assert_eq!(items, before, "合法化不得改写输入");
    }

    /// §14.1 #6 前半：契约性内容里的 Data block 按能力过滤，不报错。
    #[test]
    fn unsupported_data_blocks_are_filtered_from_tool_results() {
        let items = vec![
            assistant_calls(&[("call-1", "read")]),
            tool_result(
                "call-1",
                "read",
                vec![
                    ContentBlock::text("screenshot saved"),
                    ContentBlock::image_url("https://example.test/a.png", "image/png"),
                ],
            ),
        ];

        let normalized =
            normalize_for_request(&items, &strict_policy()).expect("normalization succeeds");

        assert_eq!(
            only_result(&normalized.messages[1]).output,
            vec![ContentBlock::text("screenshot saved")]
        );
        assert_eq!(normalized.report.filtered_data_blocks, 1);
    }

    /// §14.1 #6 后半：真实用户附上的内容不静默丢弃，返回明确错误。
    #[test]
    fn real_user_data_block_is_rejected_when_the_model_cannot_accept_it() {
        let items = vec![
            ConversationItem::real(Message::text(Role::User, "先看这个")),
            ConversationItem::real(Message {
                role: Role::User,
                content: vec![
                    ContentBlock::text("这张图"),
                    ContentBlock::image_url("https://example.test/b.png", "image/png"),
                ],
            }),
        ];

        let error = normalize_for_request(&items, &strict_policy()).expect_err("must reject");

        assert_eq!(error, NormalizationError::UnsupportedUserData { item_index: 1 });
    }

    /// D5 回归守卫：`MessageKind` 必须全程保留。
    ///
    /// Skill 正文是 `Role::User`，但不是用户输入。摊平成 `Message` 再处理的实现
    /// 会把它误判成真实用户输入并拒绝整个 Turn。
    #[test]
    fn contextual_user_items_are_filtered_rather_than_rejected() {
        let items = vec![ConversationItem::real_with_kind(
            Message {
                role: Role::User,
                content: vec![
                    ContentBlock::text("skill body"),
                    ContentBlock::image_url("https://example.test/c.png", "image/png"),
                ],
            },
            MessageKind::SkillInstruction,
        )];

        let normalized =
            normalize_for_request(&items, &strict_policy()).expect("contextual input is filtered");

        assert_eq!(
            normalized.messages,
            vec![Message {
                role: Role::User,
                content: vec![ContentBlock::text("skill body")],
            }]
        );
        assert_eq!(normalized.report.filtered_data_blocks, 1);
    }

    /// 能力允许时不得过度过滤。
    #[test]
    fn data_blocks_survive_when_the_model_accepts_them() {
        let image = ContentBlock::image_url("https://example.test/d.png", "image/png");
        let items = vec![ConversationItem::real(Message {
            role: Role::User,
            content: vec![ContentBlock::text("这张图"), image.clone()],
        })];

        let normalized =
            normalize_for_request(&items, &permissive_policy()).expect("normalization succeeds");

        assert_eq!(
            normalized.messages,
            vec![Message {
                role: Role::User,
                content: vec![ContentBlock::text("这张图"), image],
            }]
        );
        assert_eq!(normalized.report, NormalizationReport::default());
    }
}
