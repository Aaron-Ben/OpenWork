use openwork_models::model::{ContentBlock, Message};
use tokio::sync::{mpsc, oneshot};

use crate::commands::ChatStateCommand;
use crate::state::ConversationState;
use crate::{AssistantDraftSnapshot, ChatStateError, ConversationSnapshot, ConversationView};

#[derive(Clone)]
pub struct ChatStateHandle {
    command_tx: mpsc::Sender<ChatStateCommand>,
}

impl ChatStateHandle {
    pub fn spawn(initial_messages: Vec<Message>) -> Result<Self, ChatStateError> {
        let state = ConversationState::try_new(initial_messages)?;
        let (command_tx, command_rx) = mpsc::channel(64);
        tokio::spawn(run_actor(state, command_rx));
        Ok(Self { command_tx })
    }

    pub async fn append_user(&self, content: Vec<ContentBlock>) -> Result<Message, ChatStateError> {
        let (respond_to, response) = oneshot::channel();
        self.send(ChatStateCommand::AppendUser {
            content,
            respond_to,
        })
        .await?;
        response.await.map_err(|_| ChatStateError::ActorStopped)?
    }

    pub async fn append_assistant(&self, message: Message) -> Result<(), ChatStateError> {
        let (respond_to, response) = oneshot::channel();
        self.send(ChatStateCommand::AppendAssistant {
            message,
            respond_to,
        })
        .await?;
        response.await.map_err(|_| ChatStateError::ActorStopped)?
    }

    pub async fn append_tool_result(&self, message: Message) -> Result<(), ChatStateError> {
        let (respond_to, response) = oneshot::channel();
        self.send(ChatStateCommand::AppendToolResult {
            message,
            respond_to,
        })
        .await?;
        response.await.map_err(|_| ChatStateError::ActorStopped)?
    }

    pub async fn begin_draft(&self) -> Result<(), ChatStateError> {
        let (respond_to, response) = oneshot::channel();
        self.send(ChatStateCommand::BeginDraft { respond_to })
            .await?;
        response.await.map_err(|_| ChatStateError::ActorStopped)?
    }

    pub async fn apply_text_delta(&self, delta: impl Into<String>) -> Result<(), ChatStateError> {
        let (respond_to, response) = oneshot::channel();
        self.send(ChatStateCommand::ApplyTextDelta {
            delta: delta.into(),
            respond_to,
        })
        .await?;
        response.await.map_err(|_| ChatStateError::ActorStopped)?
    }

    pub async fn apply_reasoning_delta(
        &self,
        delta: impl Into<String>,
    ) -> Result<(), ChatStateError> {
        let (respond_to, response) = oneshot::channel();
        self.send(ChatStateCommand::ApplyReasoningDelta {
            delta: delta.into(),
            respond_to,
        })
        .await?;
        response.await.map_err(|_| ChatStateError::ActorStopped)?
    }

    pub async fn finish_draft(&self) -> Result<AssistantDraftSnapshot, ChatStateError> {
        let (respond_to, response) = oneshot::channel();
        self.send(ChatStateCommand::FinishDraft { respond_to })
            .await?;
        response.await.map_err(|_| ChatStateError::ActorStopped)?
    }

    pub async fn discard_draft(&self) -> Result<(), ChatStateError> {
        self.send(ChatStateCommand::DiscardDraft).await
    }

    pub async fn conversation_view(&self) -> Result<ConversationView, ChatStateError> {
        let (respond_to, response) = oneshot::channel();
        self.send(ChatStateCommand::ConversationView { respond_to })
            .await?;
        response.await.map_err(|_| ChatStateError::ActorStopped)
    }

    pub async fn snapshot(&self) -> Result<ConversationSnapshot, ChatStateError> {
        let (respond_to, response) = oneshot::channel();
        self.send(ChatStateCommand::Snapshot { respond_to }).await?;
        response.await.map_err(|_| ChatStateError::ActorStopped)
    }

    async fn send(&self, command: ChatStateCommand) -> Result<(), ChatStateError> {
        self.command_tx
            .send(command)
            .await
            .map_err(|_| ChatStateError::ActorStopped)
    }
}

async fn run_actor(mut state: ConversationState, mut command_rx: mpsc::Receiver<ChatStateCommand>) {
    while let Some(command) = command_rx.recv().await {
        match command {
            ChatStateCommand::AppendUser {
                content,
                respond_to,
            } => {
                let _ = respond_to.send(state.append_user(content));
            }
            ChatStateCommand::AppendAssistant {
                message,
                respond_to,
            } => {
                let _ = respond_to.send(state.append_assistant(message));
            }
            ChatStateCommand::AppendToolResult {
                message,
                respond_to,
            } => {
                let _ = respond_to.send(state.append_tool_result(message));
            }
            ChatStateCommand::BeginDraft { respond_to } => {
                let _ = respond_to.send(state.begin_draft());
            }
            ChatStateCommand::ApplyTextDelta { delta, respond_to } => {
                let _ = respond_to.send(state.apply_text_delta(&delta));
            }
            ChatStateCommand::ApplyReasoningDelta { delta, respond_to } => {
                let _ = respond_to.send(state.apply_reasoning_delta(&delta));
            }
            ChatStateCommand::FinishDraft { respond_to } => {
                let _ = respond_to.send(state.finish_draft());
            }
            ChatStateCommand::DiscardDraft => state.discard_draft(),
            ChatStateCommand::ConversationView { respond_to } => {
                let _ = respond_to.send(state.conversation_view());
            }
            ChatStateCommand::Snapshot { respond_to } => {
                let _ = respond_to.send(state.snapshot());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use openwork_models::model::{
        ContentBlock, Role, ToolCallBlock, ToolCallState, ToolResultBlock, ToolResultState,
    };

    use super::*;

    #[tokio::test]
    async fn serializes_conversation_writes_and_returns_a_conversation_view() {
        let chat = ChatStateHandle::spawn(Vec::new()).expect("chat state");
        chat.append_user(vec![ContentBlock::text("hello")])
            .await
            .expect("user");
        chat.begin_draft().await.expect("draft");
        chat.apply_text_delta("not committed").await.expect("delta");

        let view = chat.conversation_view().await.expect("view");
        assert_eq!(view.messages.len(), 1);
        assert_eq!(view.messages[0].role, Role::User);
    }

    #[tokio::test]
    async fn requires_tool_results_to_match_an_assistant_tool_call() {
        let chat = ChatStateHandle::spawn(Vec::new()).expect("chat state");
        chat.append_assistant(Message {
            role: Role::Assistant,
            content: vec![ContentBlock::ToolCall(ToolCallBlock {
                id: "call-1".to_string(),
                name: "read".to_string(),
                input: "{}".to_string(),
                state: ToolCallState::Submitted,
            })],
        })
        .await
        .expect("assistant");
        chat.append_tool_result(Message {
            role: Role::Tool,
            content: vec![ContentBlock::ToolResult(ToolResultBlock {
                id: "call-1".to_string(),
                name: "read".to_string(),
                output: vec![ContentBlock::text("ok")],
                state: ToolResultState::Success,
                artifacts: Vec::new(),
            })],
        })
        .await
        .expect("tool result");

        let snapshot = chat.snapshot().await.expect("snapshot");
        assert_eq!(snapshot.messages.len(), 2);
    }

    #[tokio::test]
    async fn discards_an_incomplete_streaming_draft() {
        let chat = ChatStateHandle::spawn(Vec::new()).expect("chat state");
        chat.begin_draft().await.expect("begin");
        chat.apply_text_delta("partial").await.expect("delta");
        chat.discard_draft().await.expect("discard");

        assert!(chat.snapshot().await.expect("snapshot").draft.is_none());
    }
}
