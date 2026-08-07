use super::*;

use super::plan::validate_tool_message;
use crate::plan::TurnPlan;

#[async_trait]
impl SessionStorage for PostgresStorage {
    async fn begin_turn(
        &self,
        session_id: &SessionId,
        turn_id: &TurnId,
        client_request_id: &ClientRequestId,
        model: &ResolvedModel,
        contextual_messages: &[Message],
        user_message: &Message,
    ) -> Result<(), String> {
        self.begin_turn_inner(
            session_id,
            turn_id,
            client_request_id,
            model,
            contextual_messages,
            user_message,
        )
        .await
        .map_err(|error| error.to_string())
    }

    async fn begin_model_call(
        &self,
        turn_id: &TurnId,
        model_call_index: u32,
        submission_attempt: u8,
    ) -> Result<(), String> {
        if model_call_index == 0 {
            return Err("model call index must be positive".to_string());
        }
        if submission_attempt == 0 {
            return Err("model submission attempt must be positive".to_string());
        }
        let result = sqlx::query(
            "UPDATE turns
             SET model_call_count = GREATEST(model_call_count, $2),
                 model_submission_count = model_submission_count + 1,
                 updated_at = CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai'
             WHERE id = $1 AND status = 'running'",
        )
        .bind(turn_id.as_str())
        .bind(i32::try_from(model_call_index).map_err(|_| "model call index overflow")?)
        .execute(&self.pool)
        .await
        .map_err(|error| error.to_string())?;
        if result.rows_affected() == 0 {
            return Err(StorageError::TurnNotFound(turn_id.to_string()).to_string());
        }
        Ok(())
    }

    async fn append_assistant_message(
        &self,
        turn_id: &TurnId,
        message: &Message,
        usage: Option<TokenUsage>,
    ) -> Result<String, String> {
        self.append_assistant_inner(turn_id, message, usage)
            .await
            .map_err(|error| error.to_string())
    }

    async fn append_tool_result(&self, turn_id: &TurnId, message: &Message) -> Result<(), String> {
        self.append_tool_result_inner(turn_id, message)
            .await
            .map_err(|error| error.to_string())
    }

    async fn finish_turn(
        &self,
        turn_id: &TurnId,
        outcome: &TurnOutcome,
        unfinished_plan_steps: Option<usize>,
    ) -> Result<(), String> {
        self.finish_turn_inner(turn_id, outcome, unfinished_plan_steps)
            .await
            .map_err(|error| error.to_string())
    }

    async fn load_turn_plan(&self, turn_id: &TurnId) -> Result<Option<TurnPlan>, String> {
        self.load_turn_plan_inner(turn_id)
            .await
            .map_err(|error| error.to_string())
    }

    async fn load_session_turn_plans(
        &self,
        session_id: &SessionId,
    ) -> Result<Vec<TurnPlan>, String> {
        self.load_session_turn_plans_inner(session_id)
            .await
            .map_err(|error| error.to_string())
    }

    async fn commit_plan_update(
        &self,
        turn_id: &TurnId,
        plan: &TurnPlan,
        success_tool_result: &Message,
    ) -> Result<(), String> {
        self.commit_plan_update_inner(turn_id, plan, success_tool_result)
            .await
            .map_err(|error| error.to_string())
    }

    async fn save_conversation_compaction(
        &self,
        session_id: &SessionId,
        input: NewConversationCompaction,
    ) -> Result<ConversationCompaction, String> {
        self.save_conversation_compaction_inner(session_id, input)
            .await
            .map_err(|error| error.to_string())
    }

    async fn load_compaction_source_messages(
        &self,
        session_id: &SessionId,
    ) -> Result<Vec<Message>, String> {
        self.load_message_records(session_id)
            .await
            .map(|records| {
                records
                    .into_iter()
                    .map(|record| Message {
                        role: record.role,
                        content: record.content,
                    })
                    .collect()
            })
            .map_err(|error| error.to_string())
    }

    async fn read_compaction_transcript(
        &self,
        session_id: &SessionId,
        query: ConversationTranscriptQuery,
    ) -> Result<ConversationTranscriptPage, String> {
        PostgresStorage::read_compaction_transcript(self, session_id, query)
            .await
            .map_err(|error| error.to_string())
    }

    async fn load_latest_compaction_runtime_state(
        &self,
        session_id: &SessionId,
    ) -> Result<Option<CompactionRuntimeState>, String> {
        self.load_latest_conversation_compaction(session_id)
            .await
            .map(|checkpoint| checkpoint.map(|checkpoint| checkpoint.runtime_state))
            .map_err(|error| error.to_string())
    }

    async fn rewind_conversation_compaction(
        &self,
        session_id: &SessionId,
        compaction_id: &str,
        runtime_state: CompactionRuntimeState,
        runtime_reminder: String,
    ) -> Result<ConversationCompaction, String> {
        self.rewind_conversation_compaction_inner(
            session_id,
            compaction_id,
            runtime_state,
            runtime_reminder,
        )
        .await
        .map_err(|error| error.to_string())
    }

    async fn load_compaction_last_user_message(
        &self,
        session_id: &SessionId,
        compaction_id: &str,
    ) -> Result<Message, String> {
        let compaction = self
            .load_conversation_compaction(session_id, compaction_id)
            .await
            .map_err(|error| error.to_string())?;
        self.load_compaction_last_user(&compaction)
            .await
            .map_err(|error| error.to_string())
    }

    async fn delete_conversation_compaction(
        &self,
        session_id: &SessionId,
        compaction_id: &str,
    ) -> Result<(), String> {
        self.delete_conversation_compaction_inner(session_id, compaction_id)
            .await
            .map_err(|error| error.to_string())
    }
}

impl PostgresStorage {
    async fn begin_turn_inner(
        &self,
        session_id: &SessionId,
        turn_id: &TurnId,
        client_request_id: &ClientRequestId,
        model: &ResolvedModel,
        contextual_messages: &[Message],
        user_message: &Message,
    ) -> Result<(), StorageError> {
        if user_message.role != Role::User {
            return Err(StorageError::InvalidInput(
                "begin_turn requires a user message".to_string(),
            ));
        }
        if contextual_messages
            .iter()
            .any(|message| message.role != Role::User)
        {
            return Err(StorageError::InvalidInput(
                "begin_turn contextual messages must use the user role".to_string(),
            ));
        }
        validate_resolved_model(model)?;
        let mut transaction = self.pool.begin().await?;
        lock_session(&mut transaction, session_id).await?;
        let turn_sequence = next_turn_sequence(&mut transaction, session_id).await?;
        sqlx::query(
            "INSERT INTO turns (
                 id, session_id, client_request_id, sequence, model_id,
                 resolved_provider_kind, resolved_model_name, app_version, status
             ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, 'running')",
        )
        .bind(turn_id.as_str())
        .bind(session_id.as_str())
        .bind(client_request_id.as_str())
        .bind(turn_sequence)
        .bind(&model.model_id)
        .bind(&model.provider_kind)
        .bind(&model.model_name)
        .bind(env!("CARGO_PKG_VERSION"))
        .execute(&mut *transaction)
        .await?;
        for message in contextual_messages {
            insert_message(
                &mut transaction,
                session_id,
                Some(turn_id),
                Role::User,
                serde_json::to_value(&message.content)?,
                StoredMessageKind::SkillInstruction,
                None,
            )
            .await?;
        }
        insert_message(
            &mut transaction,
            session_id,
            Some(turn_id),
            Role::User,
            serde_json::to_value(&user_message.content)?,
            StoredMessageKind::Normal,
            None,
        )
        .await?;
        sqlx::query(
            "UPDATE sessions
             SET updated_at = CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai',
                 last_turn_at = CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai'
             WHERE id = $1",
        )
        .bind(session_id.as_str())
        .execute(&mut *transaction)
        .await?;
        transaction.commit().await?;
        Ok(())
    }

    async fn append_assistant_inner(
        &self,
        turn_id: &TurnId,
        message: &Message,
        usage: Option<TokenUsage>,
    ) -> Result<String, StorageError> {
        if message.role != Role::Assistant {
            return Err(StorageError::InvalidInput(
                "append_assistant_message requires an assistant message".to_string(),
            ));
        }
        let content = serde_json::to_value(&message.content)?;
        let tool_calls = message
            .content
            .iter()
            .filter(|block| matches!(block, ContentBlock::ToolCall(_)))
            .count();
        let tool_calls = i32::try_from(tool_calls)
            .map_err(|_| StorageError::InvalidInput("tool call count overflow".to_string()))?;
        let input_tokens = optional_token(usage.and_then(|value| value.input_tokens))?;
        let output_tokens = optional_token(usage.and_then(|value| value.output_tokens))?;
        let cached_input_tokens =
            optional_token(usage.and_then(|value| value.cached_input_tokens))?;
        let reasoning_tokens = optional_token(usage.and_then(|value| value.reasoning_tokens))?;

        let mut transaction = self.pool.begin().await?;
        let session_id = lock_turn(&mut transaction, turn_id).await?;
        let message_id = insert_message(
            &mut transaction,
            &session_id,
            Some(turn_id),
            Role::Assistant,
            content,
            StoredMessageKind::Normal,
            None,
        )
        .await?;
        sqlx::query(
            "UPDATE turns SET
                 tool_call_count = tool_call_count + $2,
                 input_tokens = CASE WHEN $3::BIGINT IS NULL THEN input_tokens
                     ELSE COALESCE(input_tokens, 0) + $3 END,
                 output_tokens = CASE WHEN $4::BIGINT IS NULL THEN output_tokens
                     ELSE COALESCE(output_tokens, 0) + $4 END,
                 cached_input_tokens = CASE WHEN $5::BIGINT IS NULL THEN cached_input_tokens
                     ELSE COALESCE(cached_input_tokens, 0) + $5 END,
                 reasoning_tokens = CASE WHEN $6::BIGINT IS NULL THEN reasoning_tokens
                     ELSE COALESCE(reasoning_tokens, 0) + $6 END,
                 updated_at = CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai'
             WHERE id = $1",
        )
        .bind(turn_id.as_str())
        .bind(tool_calls)
        .bind(input_tokens)
        .bind(output_tokens)
        .bind(cached_input_tokens)
        .bind(reasoning_tokens)
        .execute(&mut *transaction)
        .await?;
        transaction.commit().await?;
        Ok(message_id)
    }

    async fn append_tool_result_inner(
        &self,
        turn_id: &TurnId,
        message: &Message,
    ) -> Result<(), StorageError> {
        let result = validate_tool_message(message)?;
        let mut transaction = self.pool.begin().await?;
        let session_id = lock_turn(&mut transaction, turn_id).await?;
        insert_message(
            &mut transaction,
            &session_id,
            Some(turn_id),
            Role::Tool,
            result.content,
            StoredMessageKind::Normal,
            Some((&result.provider_call_id, &result.tool_name)),
        )
        .await?;
        transaction.commit().await?;
        Ok(())
    }

    async fn finish_turn_inner(
        &self,
        turn_id: &TurnId,
        outcome: &TurnOutcome,
        unfinished_plan_steps: Option<usize>,
    ) -> Result<(), StorageError> {
        let unfinished_plan_steps = unfinished_plan_steps
            .map(i32::try_from)
            .transpose()
            .map_err(|_| {
                StorageError::InvalidInput(
                    "unfinished plan step count does not fit in an integer".to_string(),
                )
            })?;
        let (status, error_code, error_message) = match outcome {
            TurnOutcome::Completed { .. } => ("completed", None, None),
            TurnOutcome::Failed { code, message } => {
                ("failed", Some(code.as_str()), Some(message.as_str()))
            }
            TurnOutcome::Cancelled => ("cancelled", None, None),
        };
        let result = sqlx::query(
            "UPDATE turns
             SET status = $2, error_code = $3, error_message = $4,
                 plan_unfinished_step_count = $5,
                 ended_at = CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai',
                 updated_at = CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai'
             WHERE id = $1 AND status = 'running'",
        )
        .bind(turn_id.as_str())
        .bind(status)
        .bind(error_code)
        .bind(error_message)
        .bind(unfinished_plan_steps)
        .execute(&self.pool)
        .await?;
        if result.rows_affected() == 0 {
            return Err(StorageError::TurnNotFound(turn_id.to_string()));
        }
        Ok(())
    }
}
