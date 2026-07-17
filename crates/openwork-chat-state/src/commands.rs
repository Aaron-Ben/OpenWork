use openwork_models::model::{ContentBlock, Message, ModelRequest, ToolDefinition};
use tokio::sync::oneshot;

use crate::{AssistantDraftSnapshot, ChatStateError, ConversationSnapshot};

pub(crate) enum ChatStateCommand {
    AppendUser {
        content: Vec<ContentBlock>,
        respond_to: oneshot::Sender<Result<Message, ChatStateError>>,
    },
    AppendAssistant {
        message: Message,
        respond_to: oneshot::Sender<Result<(), ChatStateError>>,
    },
    AppendToolResult {
        message: Message,
        respond_to: oneshot::Sender<Result<(), ChatStateError>>,
    },
    BeginDraft {
        respond_to: oneshot::Sender<Result<(), ChatStateError>>,
    },
    ApplyTextDelta {
        delta: String,
        respond_to: oneshot::Sender<Result<(), ChatStateError>>,
    },
    ApplyReasoningDelta {
        delta: String,
        respond_to: oneshot::Sender<Result<(), ChatStateError>>,
    },
    FinishDraft {
        respond_to: oneshot::Sender<Result<AssistantDraftSnapshot, ChatStateError>>,
    },
    DiscardDraft,
    BuildRequest {
        model: String,
        system_prompt: String,
        tools: Vec<ToolDefinition>,
        respond_to: oneshot::Sender<ModelRequest>,
    },
    Snapshot {
        respond_to: oneshot::Sender<ConversationSnapshot>,
    },
}
