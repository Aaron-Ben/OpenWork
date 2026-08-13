use crate::ConversationItem;

/// Immutable typed Conversation input used by every model call.
#[derive(Debug, Clone, PartialEq)]
pub struct ConversationContextView {
    pub items: Vec<ConversationItem>,
}
