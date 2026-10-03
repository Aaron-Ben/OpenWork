use std::path::PathBuf;

use crate::ConversationItem;

/// Immutable typed Conversation input used by every model call.
#[derive(Debug, Clone, PartialEq)]
pub struct ConversationContextView {
    pub items: Vec<ConversationItem>,
    pub tool_result_pruning: ToolResultPruning,
}

impl ConversationContextView {
    /// A view with nothing pruned.
    pub fn new(items: Vec<ConversationItem>) -> Self {
        Self {
            items,
            tool_result_pruning: ToolResultPruning::default(),
        }
    }
}

/// Which old tool results the model projection shortens (compaction.md §2).
///
/// This is projection state, not Conversation content: the items keep their
/// full results, and every projection of the same view shortens the same
/// ones, so the request bytes stay stable.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ToolResultPruning {
    /// Tool results in persisted messages up to this sequence are pruned.
    /// Only ever increases; `None` means nothing is pruned.
    pub through_sequence: Option<i64>,
    /// Holds `<tool-call-id>.txt` with each pruned result's complete text;
    /// `None` when spilling is disabled and markers name no file.
    pub spill_directory: Option<PathBuf>,
}
