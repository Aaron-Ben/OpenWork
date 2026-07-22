use std::collections::HashSet;

use openwork_models::model::{ContentBlock, Message, Role};
use thiserror::Error;

use crate::{AssistantDraftSnapshot, ConversationSnapshot, ConversationView};

pub(crate) struct ConversationState {
    messages: Vec<Message>,
    draft: Option<AssistantDraftSnapshot>,
    unresolved_tool_calls: HashSet<String>,
}

impl ConversationState {
    pub(crate) fn try_new(messages: Vec<Message>) -> Result<Self, ChatStateError> {
        let mut state = Self {
            messages: Vec::new(),
            draft: None,
            unresolved_tool_calls: HashSet::new(),
        };
        for message in messages {
            state.append_existing(message)?;
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
        self.messages.push(message.clone());
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
        self.messages.push(message);
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
        self.messages.push(message);
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

    pub(crate) fn conversation_view(&self) -> ConversationView {
        ConversationView {
            messages: self.messages.clone(),
        }
    }

    pub(crate) fn snapshot(&self) -> ConversationSnapshot {
        ConversationSnapshot {
            messages: self.messages.clone(),
            draft: self.draft.clone(),
        }
    }

    fn append_existing(&mut self, message: Message) -> Result<(), ChatStateError> {
        match message.role {
            Role::System => return Err(ChatStateError::PersistedSystemMessage),
            Role::User => {
                if message.content.is_empty() {
                    return Err(ChatStateError::EmptyMessage);
                }
                self.messages.push(message);
            }
            Role::Assistant => self.append_assistant(message)?,
            Role::Tool => self.append_tool_result(message)?,
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
