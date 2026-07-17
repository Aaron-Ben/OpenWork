use async_trait::async_trait;
use openwork_models::model::{Message, TokenUsage};

use super::{ClientRequestId, ResolvedModel, SessionId, TurnId, TurnOutcome};

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

    async fn begin_model_call(&self, turn_id: &TurnId, model_call_index: u32)
    -> Result<(), String>;

    async fn append_assistant_message(
        &self,
        turn_id: &TurnId,
        message: &Message,
        usage: Option<TokenUsage>,
    ) -> Result<(), String>;

    async fn append_tool_result(&self, turn_id: &TurnId, message: &Message) -> Result<(), String>;

    async fn finish_turn(&self, turn_id: &TurnId, outcome: &TurnOutcome) -> Result<(), String>;
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
    ) -> Result<(), String> {
        Ok(())
    }

    async fn append_assistant_message(
        &self,
        _turn_id: &TurnId,
        _message: &Message,
        _usage: Option<TokenUsage>,
    ) -> Result<(), String> {
        Ok(())
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
}
