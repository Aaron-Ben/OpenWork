use async_trait::async_trait;
use openwork_models::model::{Message, TokenUsage};

use crate::plan::TurnPlan;
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
        contextual_messages: &[Message],
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

    async fn append_agent_message(
        &self,
        turn_id: &TurnId,
        message_id: &str,
        message: &Message,
    ) -> Result<bool, String>;

    /// 追加一条 world-state fragment（§8.4）。
    ///
    /// 一个 section 一条，`message_kind = 'world_state'`，role 恒为 User。
    /// 返回 `false` 表示 `message_id` 已存在、本次没有插入——采样在同一 Turn 内
    /// 可能重试，重复写必须是幂等的。
    async fn append_world_state_fragment(
        &self,
        turn_id: &TurnId,
        message_id: &str,
        message: &Message,
    ) -> Result<bool, String>;

    /// 收尾一个 Turn。
    ///
    /// `unfinished_plan_steps` 是 §15.1 的观测信号，`None` 表示这个 Turn 没有计划 ——
    /// 与 `Some(0)`（有计划且全部收尾）语义不同，不要在任何一层把两者合并。
    async fn finish_turn(
        &self,
        turn_id: &TurnId,
        outcome: &TurnOutcome,
        unfinished_plan_steps: Option<usize>,
    ) -> Result<(), String>;

    async fn load_turn_plan(&self, turn_id: &TurnId) -> Result<Option<TurnPlan>, String>;

    /// 一个 Session 下所有 Turn 的最终计划，供 Desktop 重建历史。
    ///
    /// 按 Session 一次取全，而不是让前端按 Turn 逐个查。
    async fn load_session_turn_plans(
        &self,
        session_id: &SessionId,
    ) -> Result<Vec<TurnPlan>, String>;

    /// 在**同一个事务**里 upsert 当前计划并追加本次成功的 Tool Result。
    ///
    /// 这条专用接口存在的唯一理由是防止两种半提交状态：计划已经改变但模型历史里没有对应的
    /// 成功 Tool Result，或者反过来。不要用两个独立的仓储方法再自行补偿——补偿代码本身
    /// 也会失败，而那时已经没有第三个地方能记录真相了。
    async fn commit_plan_update(
        &self,
        turn_id: &TurnId,
        plan: &TurnPlan,
        success_tool_result: &Message,
    ) -> Result<(), String>;

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
        _contextual_messages: &[Message],
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

    async fn append_agent_message(
        &self,
        _turn_id: &TurnId,
        _message_id: &str,
        _message: &Message,
    ) -> Result<bool, String> {
        Ok(true)
    }

    async fn append_world_state_fragment(
        &self,
        _turn_id: &TurnId,
        _message_id: &str,
        _message: &Message,
    ) -> Result<bool, String> {
        Ok(true)
    }

    async fn finish_turn(
        &self,
        _turn_id: &TurnId,
        _outcome: &TurnOutcome,
        _unfinished_plan_steps: Option<usize>,
    ) -> Result<(), String> {
        Ok(())
    }

    async fn load_turn_plan(&self, _turn_id: &TurnId) -> Result<Option<TurnPlan>, String> {
        Ok(None)
    }

    async fn load_session_turn_plans(
        &self,
        _session_id: &SessionId,
    ) -> Result<Vec<TurnPlan>, String> {
        Ok(Vec::new())
    }

    async fn commit_plan_update(
        &self,
        _turn_id: &TurnId,
        _plan: &TurnPlan,
        _success_tool_result: &Message,
    ) -> Result<(), String> {
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
