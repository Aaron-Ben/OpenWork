use openwork_models::model::{ContentBlock, Message, Role};

/// Fully materialized input passed from Core to the Session actor.
///
/// Contextual messages are model-visible but hidden from the Desktop chat
/// transcript. The user message is always stored last so compaction replays the
/// literal request rather than a preceding contextual Skill instruction.
#[derive(Debug, Clone, PartialEq)]
pub struct PreparedTurnInput {
    contextual_messages: Vec<Message>,
    user_message: Message,
}

impl PreparedTurnInput {
    pub(crate) fn new(contextual_messages: Vec<Message>, user_content: Vec<ContentBlock>) -> Self {
        Self {
            contextual_messages,
            user_message: Message {
                role: Role::User,
                content: user_content,
            },
        }
    }

    pub fn text(text: impl Into<String>) -> Self {
        Self::new(Vec::new(), vec![ContentBlock::text(text)])
    }

    pub(crate) fn contextual_messages(&self) -> &[Message] {
        &self.contextual_messages
    }

    pub(crate) fn user_message(&self) -> &Message {
        &self.user_message
    }

    pub(crate) fn into_messages(self) -> impl Iterator<Item = Message> {
        self.contextual_messages
            .into_iter()
            .chain(std::iter::once(self.user_message))
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.user_message.content.is_empty()
    }
}
