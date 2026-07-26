use openwork_models::model::Message;

use crate::ConversationItem;

/// Immutable conversation input for one model call.
///
/// Streaming drafts remain actor-owned and are intentionally excluded.
#[derive(Debug, Clone, PartialEq)]
pub struct ConversationView {
    pub messages: Vec<Message>,
}

/// Immutable typed Conversation input used by compaction and recovery.
#[derive(Debug, Clone, PartialEq)]
pub struct ConversationCompactionView {
    pub items: Vec<ConversationItem>,
}
