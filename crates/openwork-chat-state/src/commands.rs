use openwork_models::model::{ContentBlock, Message};
use tokio::sync::oneshot;

use crate::{AssistantDraftSnapshot, ChatStateError, ConversationSnapshot, ConversationView};

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
    ReplaceConversation {
        messages: Vec<Message>,
        respond_to: oneshot::Sender<Result<(), ChatStateError>>,
    },
    ConversationView {
        respond_to: oneshot::Sender<ConversationView>,
    },
    Snapshot {
        respond_to: oneshot::Sender<ConversationSnapshot>,
    },
}
