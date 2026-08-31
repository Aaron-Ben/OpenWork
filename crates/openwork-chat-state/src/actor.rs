use openwork_models::model::{ContentBlock, Message};
use tokio::sync::{mpsc, oneshot};

use crate::commands::ChatStateCommand;
use crate::state::ConversationState;
use crate::{
    AssistantDraftSnapshot, ChatStateError, ConversationContextView, ConversationItem,
    ConversationSnapshot, MessageKind,
};

#[derive(Clone)]
pub struct ChatStateHandle {
    command_tx: mpsc::Sender<ChatStateCommand>,
}

impl ChatStateHandle {
    pub fn spawn(initial_messages: Vec<Message>) -> Result<Self, ChatStateError> {
        let state = ConversationState::try_new(initial_messages)?;
        Ok(Self::spawn_state(state))
    }

    pub fn spawn_items(initial_items: Vec<ConversationItem>) -> Result<Self, ChatStateError> {
        let state = ConversationState::try_new_items(initial_items)?;
        Ok(Self::spawn_state(state))
    }

    fn spawn_state(state: ConversationState) -> Self {
        let (command_tx, command_rx) = mpsc::channel(64);
        tokio::spawn(run_actor(state, command_rx));
        Self { command_tx }
    }

    pub async fn append_user(&self, content: Vec<ContentBlock>) -> Result<Message, ChatStateError> {
        self.append_user_with_kind(content, MessageKind::Normal)
            .await
    }

    /// Appends a User-role message while preserving what the message means.
    ///
    /// Skill instructions and sub-agent messages share the provider-facing User
    /// role with real requests, so live Conversation state must retain their
    /// kind just like the resume path does.
    pub async fn append_user_with_kind(
        &self,
        content: Vec<ContentBlock>,
        kind: MessageKind,
    ) -> Result<Message, ChatStateError> {
        let (respond_to, response) = oneshot::channel();
        self.send(ChatStateCommand::AppendUser {
            content,
            kind,
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

    /// Atomically replaces the committed Conversation after validating the
    /// complete replacement. The previous Conversation remains unchanged when
    /// validation fails.
    pub async fn replace_conversation(&self, messages: Vec<Message>) -> Result<(), ChatStateError> {
        let (respond_to, response) = oneshot::channel();
        self.send(ChatStateCommand::ReplaceConversation {
            messages,
            respond_to,
        })
        .await?;
        response.await.map_err(|_| ChatStateError::ActorStopped)?
    }

    /// Atomically replaces the committed Conversation with typed items used by
    /// compaction/recovery while preserving provider-facing Message roles.
    pub async fn replace_items(&self, items: Vec<ConversationItem>) -> Result<(), ChatStateError> {
        let (respond_to, response) = oneshot::channel();
        self.send(ChatStateCommand::ReplaceItems { items, respond_to })
            .await?;
        response.await.map_err(|_| ChatStateError::ActorStopped)?
    }

    pub async fn context_view(&self) -> Result<ConversationContextView, ChatStateError> {
        let (respond_to, response) = oneshot::channel();
        self.send(ChatStateCommand::ContextView { respond_to })
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
                kind,
                respond_to,
            } => {
                let _ = respond_to.send(state.append_user(content, kind));
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
            ChatStateCommand::ReplaceConversation {
                messages,
                respond_to,
            } => {
                let _ = respond_to.send(state.replace_conversation(messages));
            }
            ChatStateCommand::ReplaceItems { items, respond_to } => {
                let _ = respond_to.send(state.replace_items(items));
            }
            ChatStateCommand::ContextView { respond_to } => {
                let _ = respond_to.send(state.context_view());
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
    async fn serializes_conversation_writes_and_returns_a_context_view() {
        let chat = ChatStateHandle::spawn(Vec::new()).expect("chat state");
        chat.append_user(vec![ContentBlock::text("hello")])
            .await
            .expect("user");
        chat.begin_draft().await.expect("draft");
        chat.apply_text_delta("not committed").await.expect("delta");

        let view = chat.context_view().await.expect("view");
        assert_eq!(view.items.len(), 1);
        assert_eq!(view.items[0].message.role, Role::User);
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

    #[tokio::test]
    async fn replaces_the_committed_conversation_atomically() {
        let chat =
            ChatStateHandle::spawn(vec![Message::text(Role::User, "old")]).expect("chat state");

        chat.replace_conversation(vec![Message::text(Role::User, "summary")])
            .await
            .expect("replacement");

        assert_eq!(
            chat.snapshot().await.expect("snapshot").messages,
            [Message::text(Role::User, "summary")]
        );
    }

    #[tokio::test]
    async fn keeps_the_previous_conversation_when_replacement_is_invalid() {
        let chat =
            ChatStateHandle::spawn(vec![Message::text(Role::User, "old")]).expect("chat state");
        let invalid = Message {
            role: Role::Tool,
            content: vec![ContentBlock::ToolResult(ToolResultBlock {
                id: "orphan".to_string(),
                name: "read".to_string(),
                output: vec![ContentBlock::text("result")],
                state: ToolResultState::Success,
                artifacts: Vec::new(),
            })],
        };

        assert!(matches!(
            chat.replace_conversation(vec![invalid]).await,
            Err(ChatStateError::UnmatchedToolResult(id)) if id == "orphan"
        ));
        assert_eq!(
            chat.snapshot().await.expect("snapshot").messages,
            [Message::text(Role::User, "old")]
        );
    }
}
