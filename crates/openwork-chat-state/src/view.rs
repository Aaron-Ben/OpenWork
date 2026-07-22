use openwork_models::model::Message;

/// Immutable conversation input for one model call.
///
/// Streaming drafts remain actor-owned and are intentionally excluded.
#[derive(Debug, Clone, PartialEq)]
pub struct ConversationView {
    pub messages: Vec<Message>,
}
