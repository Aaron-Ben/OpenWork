use std::collections::VecDeque;
use std::sync::Arc;

use openwork_models::model::{Message, Role};
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;

use super::SessionId;
use crate::agent::AgentControl;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentMessageKind {
    FinalAnswer,
    Interrupted,
    Failed,
}

impl AgentMessageKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::FinalAnswer => "final_answer",
            Self::Interrupted => "interrupted",
            Self::Failed => "failed",
        }
    }
}

#[derive(Clone)]
pub struct ParentLink {
    pub parent_session_id: SessionId,
    pub task_name: String,
    pub agent_control: AgentControl,
}

impl std::fmt::Debug for ParentLink {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ParentLink")
            .field("parent_session_id", &self.parent_session_id)
            .field("task_name", &self.task_name)
            .finish_non_exhaustive()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct AgentMessage {
    pub task_name: String,
    pub kind: AgentMessageKind,
    pub body: String,
}

impl AgentMessage {
    pub fn into_model_message(self) -> Message {
        Message::text(
            Role::User,
            format!(
                "<agent_message>\n<task>{}</task>\n<kind>{}</kind>\n<body>\n{}\n</body>\n</agent_message>",
                self.task_name,
                self.kind.as_str(),
                self.body
            ),
        )
    }
}

/// In-memory queue of sub-agent results that have not entered the parent
/// Conversation yet. Delivery never starts a Turn; only a running Turn drains
/// the queue before its next Model Call.
#[derive(Debug, Clone, Default)]
pub(super) struct AgentMailbox {
    pending: Arc<Mutex<VecDeque<AgentMessage>>>,
}

impl AgentMailbox {
    pub async fn push(&self, message: AgentMessage) {
        self.pending.lock().await.push_back(message);
    }

    pub async fn front(&self) -> Option<AgentMessage> {
        self.pending.lock().await.front().cloned()
    }

    pub async fn pop_front(&self) {
        self.pending.lock().await.pop_front();
    }
}

#[cfg(test)]
mod tests {
    use openwork_models::model::ContentBlock;

    use super::*;

    #[test]
    fn final_answer_uses_the_agent_message_envelope_and_user_role() {
        let message = AgentMessage {
            task_name: "find_auth_flow".to_string(),
            kind: AgentMessageKind::FinalAnswer,
            body: "Found it.".to_string(),
        }
        .into_model_message();

        assert_eq!(message.role, Role::User);
        assert_eq!(
            message.content,
            [ContentBlock::text(
                "<agent_message>\n<task>find_auth_flow</task>\n<kind>final_answer</kind>\n<body>\nFound it.\n</body>\n</agent_message>"
            )]
        );
    }
}
