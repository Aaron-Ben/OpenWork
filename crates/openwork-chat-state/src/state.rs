use std::collections::HashSet;

use openwork_models::model::{ContentBlock, Message, Role};
use thiserror::Error;

use crate::{
    AssistantDraftSnapshot, ConversationCompactionView, ConversationItem, ConversationSnapshot,
    ConversationView,
};

pub(crate) struct ConversationState {
    items: Vec<ConversationItem>,
    draft: Option<AssistantDraftSnapshot>,
    unresolved_tool_calls: HashSet<String>,
}

impl ConversationState {
    pub(crate) fn try_new(messages: Vec<Message>) -> Result<Self, ChatStateError> {
        Self::try_new_items(messages.into_iter().map(ConversationItem::real).collect())
    }

    pub(crate) fn try_new_items(items: Vec<ConversationItem>) -> Result<Self, ChatStateError> {
        let mut state = Self {
            items: Vec::new(),
            draft: None,
            unresolved_tool_calls: HashSet::new(),
        };
        for item in items {
            state.append_existing(item)?;
        }
        Ok(state)
    }

    pub(crate) fn append_user(
        &mut self,
        content: Vec<ContentBlock>,
    ) -> Result<Message, ChatStateError> {
        if content.is_empty() {
            return Err(ChatStateError::EmptyMessage);
        }
        let message = Message {
            role: Role::User,
            content,
        };
        self.items.push(ConversationItem::real(message.clone()));
        Ok(message)
    }

    pub(crate) fn append_assistant(&mut self, message: Message) -> Result<(), ChatStateError> {
        if message.role != Role::Assistant {
            return Err(ChatStateError::UnexpectedRole {
                expected: Role::Assistant,
                actual: message.role,
            });
        }
        self.register_tool_calls(&message)?;
        self.items.push(ConversationItem::real(message));
        Ok(())
    }

    pub(crate) fn append_tool_result(&mut self, message: Message) -> Result<(), ChatStateError> {
        if message.role != Role::Tool {
            return Err(ChatStateError::UnexpectedRole {
                expected: Role::Tool,
                actual: message.role,
            });
        }
        if message.content.is_empty() {
            return Err(ChatStateError::EmptyMessage);
        }
        for block in &message.content {
            let ContentBlock::ToolResult(result) = block else {
                return Err(ChatStateError::InvalidToolMessage);
            };
            if !self.unresolved_tool_calls.remove(&result.id) {
                return Err(ChatStateError::UnmatchedToolResult(result.id.clone()));
            }
        }
        self.items.push(ConversationItem::real(message));
        Ok(())
    }

    pub(crate) fn begin_draft(&mut self) -> Result<(), ChatStateError> {
        if self.draft.is_some() {
            return Err(ChatStateError::DraftAlreadyActive);
        }
        self.draft = Some(AssistantDraftSnapshot::default());
        Ok(())
    }

    pub(crate) fn apply_text_delta(&mut self, delta: &str) -> Result<(), ChatStateError> {
        self.draft
            .as_mut()
            .ok_or(ChatStateError::NoActiveDraft)?
            .text
            .push_str(delta);
        Ok(())
    }

    pub(crate) fn apply_reasoning_delta(&mut self, delta: &str) -> Result<(), ChatStateError> {
        self.draft
            .as_mut()
            .ok_or(ChatStateError::NoActiveDraft)?
            .reasoning
            .push_str(delta);
        Ok(())
    }

    pub(crate) fn finish_draft(&mut self) -> Result<AssistantDraftSnapshot, ChatStateError> {
        self.draft.take().ok_or(ChatStateError::NoActiveDraft)
    }

    pub(crate) fn discard_draft(&mut self) {
        self.draft = None;
    }

    pub(crate) fn replace_conversation(
        &mut self,
        messages: Vec<Message>,
    ) -> Result<(), ChatStateError> {
        if self.draft.is_some() {
            return Err(ChatStateError::DraftAlreadyActive);
        }
        let replacement = Self::try_new(messages)?;
        *self = replacement;
        Ok(())
    }

    pub(crate) fn replace_items(
        &mut self,
        items: Vec<ConversationItem>,
    ) -> Result<(), ChatStateError> {
        if self.draft.is_some() {
            return Err(ChatStateError::DraftAlreadyActive);
        }
        let replacement = Self::try_new_items(items)?;
        *self = replacement;
        Ok(())
    }

    pub(crate) fn conversation_view(&self) -> ConversationView {
        ConversationView {
            messages: self.items.iter().map(|item| item.message.clone()).collect(),
        }
    }

    pub(crate) fn compaction_view(&self) -> ConversationCompactionView {
        ConversationCompactionView {
            items: self.items.clone(),
        }
    }

    pub(crate) fn snapshot(&self) -> ConversationSnapshot {
        ConversationSnapshot {
            messages: self.items.iter().map(|item| item.message.clone()).collect(),
            draft: self.draft.clone(),
        }
    }

    fn append_existing(&mut self, item: ConversationItem) -> Result<(), ChatStateError> {
        match item.message.role {
            Role::System => return Err(ChatStateError::PersistedSystemMessage),
            Role::User => {
                if item.message.content.is_empty() {
                    return Err(ChatStateError::EmptyMessage);
                }
                self.items.push(item);
            }
            Role::Assistant => {
                self.register_tool_calls(&item.message)?;
                self.items.push(item);
            }
            Role::Tool => {
                if item.message.content.is_empty() {
                    return Err(ChatStateError::EmptyMessage);
                }
                for block in &item.message.content {
                    let ContentBlock::ToolResult(result) = block else {
                        return Err(ChatStateError::InvalidToolMessage);
                    };
                    if !self.unresolved_tool_calls.remove(&result.id) {
                        return Err(ChatStateError::UnmatchedToolResult(result.id.clone()));
                    }
                }
                self.items.push(item);
            }
        }
        Ok(())
    }

    fn register_tool_calls(&mut self, message: &Message) -> Result<(), ChatStateError> {
        for block in &message.content {
            if let ContentBlock::ToolCall(call) = block
                && !self.unresolved_tool_calls.insert(call.id.clone())
            {
                return Err(ChatStateError::DuplicateToolCall(call.id.clone()));
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ChatStateError {
    #[error("chat state actor stopped")]
    ActorStopped,
    #[error("message content must not be empty")]
    EmptyMessage,
    #[error("persisted conversation must not contain a system message")]
    PersistedSystemMessage,
    #[error("unexpected message role: expected {expected:?}, got {actual:?}")]
    UnexpectedRole { expected: Role, actual: Role },
    #[error("tool messages must contain only tool result blocks")]
    InvalidToolMessage,
    #[error("tool result has no matching assistant tool call: {0}")]
    UnmatchedToolResult(String),
    #[error("duplicate unresolved tool call: {0}")]
    DuplicateToolCall(String),
    #[error("assistant draft is already active")]
    DraftAlreadyActive,
    #[error("assistant draft is not active")]
    NoActiveDraft,
}
