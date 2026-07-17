use openwork_models::model::Message;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AssistantDraftSnapshot {
    pub text: String,
    pub reasoning: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ConversationSnapshot {
    pub messages: Vec<Message>,
    pub draft: Option<AssistantDraftSnapshot>,
}
