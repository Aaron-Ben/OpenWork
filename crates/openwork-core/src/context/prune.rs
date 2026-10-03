//! 旧工具结果修剪（compaction.md §2）。
//!
//! 水位线以下、过长的 Tool Result 在投影里只保留开头与结尾，中间换成一行
//! 标记，指明完整内容所在的落盘文件。这是纯投影变换：`messages` 里的原文
//! 不变，同一个视图每次投影都得到逐字节相同的结果。

use std::path::Path;

use openwork_chat_state::{ConversationItem, ConversationItemOrigin, ToolResultPruning};
use openwork_models::model::{ContentBlock, Role, ToolResultBlock};
use openwork_tools::SpillDirectory;

/// 超过这么多字符的 Tool Result 才修剪。
pub(crate) const PRUNE_ABOVE_CHARS: usize = 8192;
const KEEP_HEAD_CHARS: usize = 4096;
const KEEP_TAIL_CHARS: usize = 1024;

/// 按视图的水位线修剪；水位线为空时原样返回。
pub(crate) fn prune_tool_results(
    items: &[ConversationItem],
    pruning: &ToolResultPruning,
) -> Vec<ConversationItem> {
    let Some(through) = pruning.through_sequence else {
        return items.to_vec();
    };
    items
        .iter()
        .map(|item| {
            if !is_pruned(item, through) {
                return item.clone();
            }
            let mut item = item.clone();
            for block in &mut item.message.content {
                if let ContentBlock::ToolResult(result) = block {
                    prune_result(result, pruning.spill_directory.as_deref());
                }
            }
            item
        })
        .collect()
}

/// 水位线覆盖的是**已落库**的 Tool 消息：只有它们有序号。运行中新追加的
/// 消息还没有序号，它们一定比水位线新。
fn is_pruned(item: &ConversationItem, through: i64) -> bool {
    item.message.role == Role::Tool
        && matches!(
            item.origin,
            ConversationItemOrigin::Real {
                sequence: Some(sequence),
                ..
            } if sequence <= through
        )
}

/// The text a pruned result would be cut from, when it is long enough to
/// prune. Results that carry data blocks keep them; only the text shrinks.
pub(crate) fn prunable_text(result: &ToolResultBlock) -> Option<String> {
    let text = result
        .output
        .iter()
        .filter_map(|block| match block {
            ContentBlock::Text(text) => Some(text.text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n");
    (text.chars().count() > PRUNE_ABOVE_CHARS).then_some(text)
}

fn prune_result(result: &mut ToolResultBlock, spill_directory: Option<&Path>) {
    let Some(text) = prunable_text(result) else {
        return;
    };
    let total = text.chars().count();
    let head_end = char_offset(&text, KEEP_HEAD_CHARS);
    let tail_start = char_offset(&text, total - KEEP_TAIL_CHARS);
    let omitted = total - KEEP_HEAD_CHARS - KEEP_TAIL_CHARS;
    let marker = match spill_directory {
        Some(directory) => format!(
            "... [tool result pruned: {omitted} characters omitted. Full result at {} — read it if you still need it]",
            spill_file(directory, &result.id).display()
        ),
        None => format!("... [tool result pruned: {omitted} characters omitted]"),
    };
    let pruned = format!("{}\n{marker}\n{}", &text[..head_end], &text[tail_start..]);
    let mut replaced = false;
    result.output.retain_mut(|block| {
        let ContentBlock::Text(text) = block else {
            return true;
        };
        if replaced {
            return false;
        }
        replaced = true;
        text.text = pruned.clone();
        true
    });
}

/// The spill file named in the marker; the tools write bounded results to the
/// same path (tools.md §8).
pub(crate) fn spill_file(directory: &Path, tool_call_id: &str) -> std::path::PathBuf {
    SpillDirectory::new(directory).file_for_call(tool_call_id)
}

fn char_offset(text: &str, chars: usize) -> usize {
    text.char_indices()
        .nth(chars)
        .map_or(text.len(), |(offset, _)| offset)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use openwork_models::model::{Message, ToolResultState};

    use super::*;

    fn tool_item(sequence: Option<i64>, id: &str, text: &str) -> ConversationItem {
        ConversationItem {
            origin: ConversationItemOrigin::Real {
                message_id: sequence.map(|sequence| format!("message-{sequence}")),
                sequence,
            },
            kind: openwork_chat_state::MessageKind::Normal,
            message: Message {
                role: Role::Tool,
                content: vec![ContentBlock::ToolResult(ToolResultBlock {
                    id: id.to_string(),
                    name: "read".to_string(),
                    output: vec![ContentBlock::text(text)],
                    state: ToolResultState::Success,
                    artifacts: Vec::new(),
                })],
            },
        }
    }

    fn text_of(item: &ConversationItem) -> &str {
        match &item.message.content[0] {
            ContentBlock::ToolResult(result) => match &result.output[0] {
                ContentBlock::Text(text) => &text.text,
                _ => panic!("text output"),
            },
            _ => panic!("tool result"),
        }
    }

    fn pruning(through: i64) -> ToolResultPruning {
        ToolResultPruning {
            through_sequence: Some(through),
            spill_directory: Some(PathBuf::from("/spill/session")),
        }
    }

    #[test]
    fn long_results_below_the_watermark_keep_head_and_tail() {
        let long = format!(
            "{}{}{}",
            "h".repeat(5000),
            "m".repeat(10_000),
            "t".repeat(2000)
        );
        let items = vec![tool_item(Some(3), "call-1", &long)];

        let pruned = prune_tool_results(&items, &pruning(3));

        let text = text_of(&pruned[0]);
        assert!(text.starts_with(&"h".repeat(4096)));
        assert!(text.ends_with(&"t".repeat(1024)));
        assert!(text.contains(
            "... [tool result pruned: 11880 characters omitted. Full result at /spill/session/call-1.txt — read it if you still need it]"
        ));
        assert_eq!(text_of(&items[0]), long, "the original stays intact");
    }

    #[test]
    fn short_newer_and_live_results_are_untouched() {
        let long = "x".repeat(PRUNE_ABOVE_CHARS + 1);
        let items = vec![
            tool_item(Some(1), "short", &"s".repeat(PRUNE_ABOVE_CHARS)),
            tool_item(Some(5), "newer", &long),
            tool_item(None, "live", &long),
        ];

        let pruned = prune_tool_results(&items, &pruning(4));

        assert_eq!(pruned, items);
    }

    #[test]
    fn pruning_is_byte_stable_and_counts_characters() {
        let long = "中".repeat(9000);
        let items = vec![tool_item(Some(2), "call-cjk", &long)];

        let first = prune_tool_results(&items, &pruning(2));
        let second = prune_tool_results(&items, &pruning(2));

        assert_eq!(first, second);
        let text = text_of(&first[0]);
        assert!(text.starts_with(&"中".repeat(4096)));
        assert!(text.contains("3880 characters omitted"));
    }

    #[test]
    fn without_a_spill_directory_the_marker_names_no_file() {
        let items = vec![tool_item(Some(1), "call-1", &"x".repeat(10_000))];
        let pruned = prune_tool_results(
            &items,
            &ToolResultPruning {
                through_sequence: Some(1),
                spill_directory: None,
            },
        );
        assert!(
            text_of(&pruned[0]).contains("... [tool result pruned: 4880 characters omitted]\n")
        );
    }
}
