use async_trait::async_trait;
use openwork_models::model::{Message, TokenUsage};

use crate::storage::{ConversationTranscriptPage, ConversationTranscriptQuery};

use super::{
    ClientRequestId, ConversationCompaction, NewConversationCompaction, ResolvedModel, SessionId,
    TurnId, TurnOutcome,
};

#[async_trait]
pub trait SessionStorage: Send + Sync {
    async fn begin_turn(
        &self,
        session_id: &SessionId,
        turn_id: &TurnId,
        client_request_id: &ClientRequestId,
        model: &ResolvedModel,
        user_message: &Message,
    ) -> Result<(), String>;

    async fn begin_model_call(
        &self,
        turn_id: &TurnId,
        model_call_index: u32,
        submission_attempt: u8,
    ) -> Result<(), String>;

    async fn append_assistant_message(
        &self,
        turn_id: &TurnId,
        message: &Message,
        usage: Option<TokenUsage>,
    ) -> Result<String, String>;

    async fn append_tool_result(&self, turn_id: &TurnId, message: &Message) -> Result<(), String>;

    async fn finish_turn(&self, turn_id: &TurnId, outcome: &TurnOutcome) -> Result<(), String>;

    async fn save_conversation_compaction(
        &self,
        session_id: &SessionId,
        input: NewConversationCompaction,
    ) -> Result<ConversationCompaction, String>;

    async fn load_compaction_source_messages(
        &self,
        session_id: &SessionId,
    ) -> Result<Vec<Message>, String>;

    async fn read_compaction_transcript(
        &self,
        _session_id: &SessionId,
        _query: ConversationTranscriptQuery,
    ) -> Result<ConversationTranscriptPage, String> {
        Err("compaction transcript readback requires durable storage".to_string())
    }

    async fn load_latest_compaction_runtime_state(
        &self,
        session_id: &SessionId,
    ) -> Result<Option<super::CompactionRuntimeState>, String>;

    async fn rewind_conversation_compaction(
        &self,
        session_id: &SessionId,
        compaction_id: &str,
        runtime_state: super::CompactionRuntimeState,
        runtime_reminder: String,
    ) -> Result<ConversationCompaction, String>;

    async fn load_compaction_last_user_message(
        &self,
        session_id: &SessionId,
        compaction_id: &str,
    ) -> Result<Message, String>;

    async fn delete_conversation_compaction(
        &self,
        session_id: &SessionId,
        compaction_id: &str,
    ) -> Result<(), String>;
}

#[derive(Debug, Default)]
pub struct NoopSessionStorage;

#[async_trait]
impl SessionStorage for NoopSessionStorage {
    async fn begin_turn(
        &self,
        _session_id: &SessionId,
        _turn_id: &TurnId,
        _client_request_id: &ClientRequestId,
        _model: &ResolvedModel,
        _user_message: &Message,
    ) -> Result<(), String> {
        Ok(())
    }

    async fn begin_model_call(
        &self,
        _turn_id: &TurnId,
        _model_call_index: u32,
        _submission_attempt: u8,
    ) -> Result<(), String> {
        Ok(())
    }

    async fn append_assistant_message(
        &self,
        _turn_id: &TurnId,
        _message: &Message,
        _usage: Option<TokenUsage>,
    ) -> Result<String, String> {
        Ok("msg-noop".to_string())
    }

    async fn append_tool_result(
        &self,
        _turn_id: &TurnId,
        _message: &Message,
    ) -> Result<(), String> {
        Ok(())
    }

    async fn finish_turn(&self, _turn_id: &TurnId, _outcome: &TurnOutcome) -> Result<(), String> {
        Ok(())
    }

    async fn save_conversation_compaction(
        &self,
        session_id: &SessionId,
        input: NewConversationCompaction,
    ) -> Result<ConversationCompaction, String> {
        Ok(ConversationCompaction::in_memory(session_id, &input))
    }

    async fn load_compaction_source_messages(
        &self,
        _session_id: &SessionId,
    ) -> Result<Vec<Message>, String> {
        Ok(Vec::new())
    }

    async fn load_latest_compaction_runtime_state(
        &self,
        _session_id: &SessionId,
    ) -> Result<Option<super::CompactionRuntimeState>, String> {
        Ok(None)
    }

    async fn rewind_conversation_compaction(
        &self,
        _session_id: &SessionId,
        _compaction_id: &str,
        _runtime_state: super::CompactionRuntimeState,
        _runtime_reminder: String,
    ) -> Result<ConversationCompaction, String> {
        Err("conversation rewind requires durable storage".to_string())
    }

    async fn load_compaction_last_user_message(
        &self,
        _session_id: &SessionId,
        _compaction_id: &str,
    ) -> Result<Message, String> {
        Err("conversation checkpoint lookup requires durable storage".to_string())
    }

    async fn delete_conversation_compaction(
        &self,
        _session_id: &SessionId,
        _compaction_id: &str,
    ) -> Result<(), String> {
        Ok(())
    }
}
