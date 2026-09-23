//! 修剪先于摘要（compaction.md §1.1）。
//!
//! threshold 与 overflow 触发后，先把修剪水位线推进到最近一次模型响应，
//! 再由调用方重新估算；修剪后已低于阈值就不调摘要模型。任何一步失败都
//! 只是"这次不修剪"，调用方照常走摘要——修剪是省钱的捷径，不是新的失败点。

use std::path::Path;

use openwork_chat_state::{
    ChatStateHandle, ConversationItem, ConversationItemOrigin, ToolResultPruning,
};
use openwork_models::model::{ContentBlock, Role};

use super::super::{SessionId, SessionStorage};
use crate::context::{prunable_text, spill_file};

/// Moves the pruning watermark up to the latest model response and installs
/// it in the Chat State. Returns whether any new result is now pruned.
pub(crate) async fn advance_tool_result_pruning(
    storage: &dyn SessionStorage,
    chat: &ChatStateHandle,
    session_id: &SessionId,
) -> bool {
    match try_advance(storage, chat, session_id).await {
        Ok(advanced) => advanced,
        Err(error) => {
            tracing::warn!(
                session_id = %session_id,
                error = %error,
                "tool result pruning skipped; falling back to summary compaction"
            );
            false
        }
    }
}

async fn try_advance(
    storage: &dyn SessionStorage,
    chat: &ChatStateHandle,
    session_id: &SessionId,
) -> Result<bool, String> {
    let current = chat
        .context_view()
        .await
        .map_err(|error| error.to_string())?
        .tool_result_pruning;
    // Items appended during this run carry no sequence yet; the persisted
    // copy does, and it is the same Conversation.
    let items = storage.load_conversation_items(session_id).await?;
    let Some(plan) = plan_pruning(&items, current.through_sequence) else {
        return Ok(false);
    };
    if let Some(directory) = &current.spill_directory {
        for (tool_call_id, text) in &plan.newly_pruned {
            ensure_spilled(directory, tool_call_id, text).await?;
        }
    }
    // Install the sequenced items first: if that fails nothing is persisted,
    // so storage and the Chat State never disagree about the watermark.
    chat.replace_items(items)
        .await
        .map_err(|error| error.to_string())?;
    let through_sequence = storage
        .advance_tool_result_pruning(session_id, plan.through_sequence)
        .await?;
    chat.set_tool_result_pruning(ToolResultPruning {
        through_sequence: Some(through_sequence),
        spill_directory: current.spill_directory,
    })
    .await
    .map_err(|error| error.to_string())?;
    Ok(true)
}

#[derive(Debug, PartialEq, Eq)]
struct PruningPlan {
    through_sequence: i64,
    /// `(tool call id, full text)` of results the new watermark starts to prune.
    newly_pruned: Vec<(String, String)>,
}

/// The watermark stops at the latest model response: the results it produced
/// are what the model works from next, so they are never pruned. `None` when
/// moving the watermark would prune nothing new.
fn plan_pruning(items: &[ConversationItem], current: Option<i64>) -> Option<PruningPlan> {
    let through_sequence = items
        .iter()
        .filter(|item| item.message.role == Role::Assistant)
        .filter_map(sequence)
        .max()?;
    let newly_pruned = items
        .iter()
        .filter(|item| item.message.role == Role::Tool)
        .filter(|item| {
            sequence(item).is_some_and(|sequence| {
                sequence <= through_sequence && current.is_none_or(|current| sequence > current)
            })
        })
        .flat_map(|item| &item.message.content)
        .filter_map(|block| match block {
            ContentBlock::ToolResult(result) => {
                prunable_text(result).map(|text| (result.id.clone(), text))
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    (!newly_pruned.is_empty()).then_some(PruningPlan {
        through_sequence,
        newly_pruned,
    })
}

fn sequence(item: &ConversationItem) -> Option<i64> {
    match item.origin {
        ConversationItemOrigin::Real { sequence, .. } => sequence,
        ConversationItemOrigin::Synthetic { .. } => None,
    }
}

/// The marker names `<spill>/<tool-call-id>.txt`, so it must be readable.
/// Tools already spilled their large results there; smaller ones are written
/// now. An existing file is kept: for bash it holds more than the result.
async fn ensure_spilled(directory: &Path, tool_call_id: &str, text: &str) -> Result<(), String> {
    let path = spill_file(directory, tool_call_id);
    if tokio::fs::try_exists(&path).await.unwrap_or(false) {
        return Ok(());
    }
    tokio::fs::create_dir_all(directory)
        .await
        .map_err(|error| format!("failed to create {}: {error}", directory.display()))?;
    tokio::fs::write(&path, text)
        .await
        .map_err(|error| format!("failed to write {}: {error}", path.display()))
}

#[cfg(test)]
mod tests {
    use openwork_chat_state::MessageKind;
    use openwork_models::model::{
        Message, ToolCallBlock, ToolCallState, ToolResultBlock, ToolResultState,
    };

    use super::*;

    fn persisted(sequence: i64, message: Message) -> ConversationItem {
        ConversationItem::persisted_with_kind(
            format!("message-{sequence}"),
            sequence,
            MessageKind::Normal,
            message,
        )
    }

    fn assistant(sequence: i64, call_id: &str) -> ConversationItem {
        persisted(
            sequence,
            Message {
                role: Role::Assistant,
                content: vec![ContentBlock::ToolCall(ToolCallBlock {
                    id: call_id.to_string(),
                    name: "read".to_string(),
                    input: "{}".to_string(),
                    state: ToolCallState::Finished,
                })],
            },
        )
    }

    fn result(sequence: i64, call_id: &str, chars: usize) -> ConversationItem {
        persisted(
            sequence,
            Message {
                role: Role::Tool,
                content: vec![ContentBlock::ToolResult(ToolResultBlock {
                    id: call_id.to_string(),
                    name: "read".to_string(),
                    output: vec![ContentBlock::text("x".repeat(chars))],
                    state: ToolResultState::Success,
                    artifacts: Vec::new(),
                })],
            },
        )
    }

    fn ids(plan: &PruningPlan) -> Vec<&str> {
        plan.newly_pruned
            .iter()
            .map(|(id, _)| id.as_str())
            .collect()
    }

    #[test]
    fn stops_before_the_latest_model_response_and_skips_short_results() {
        let items = vec![
            assistant(1, "old-long"),
            result(2, "old-long", 9000),
            assistant(3, "old-short"),
            result(4, "old-short", 100),
            assistant(5, "latest"),
            result(6, "latest", 9000),
        ];

        let plan = plan_pruning(&items, None).expect("something to prune");

        assert_eq!(plan.through_sequence, 5);
        assert_eq!(ids(&plan), ["old-long"]);
    }

    #[test]
    fn only_results_above_the_current_watermark_are_new() {
        let items = vec![
            assistant(1, "first"),
            result(2, "first", 9000),
            assistant(3, "second"),
            result(4, "second", 9000),
            assistant(5, "latest"),
            result(6, "latest", 9000),
        ];

        let plan = plan_pruning(&items, Some(3)).expect("second is new");

        assert_eq!(ids(&plan), ["second"]);
        assert_eq!(plan_pruning(&items, Some(5)), None);
    }

    #[test]
    fn nothing_long_enough_means_no_plan() {
        let items = vec![assistant(1, "a"), result(2, "a", 100), assistant(3, "b")];
        assert_eq!(plan_pruning(&items, None), None);
    }

    #[tokio::test]
    async fn existing_spill_files_are_kept_and_missing_ones_written() {
        let directory = std::env::temp_dir().join(format!(
            "openwork-prune-spill-{}",
            uuid::Uuid::new_v4().simple()
        ));
        std::fs::create_dir_all(&directory).expect("directory");
        std::fs::write(directory.join("bash-call.txt"), "complete bash output").expect("spill");

        ensure_spilled(&directory, "bash-call", "bounded result")
            .await
            .expect("kept");
        ensure_spilled(&directory, "read-call", "read result")
            .await
            .expect("written");

        assert_eq!(
            std::fs::read_to_string(directory.join("bash-call.txt")).unwrap(),
            "complete bash output"
        );
        assert_eq!(
            std::fs::read_to_string(directory.join("read-call.txt")).unwrap(),
            "read result"
        );
        let _ = std::fs::remove_dir_all(directory);
    }
}
