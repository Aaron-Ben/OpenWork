use super::*;

impl PostgresStorage {
    pub async fn read_compaction_transcript(
        &self,
        session_id: &SessionId,
        query: ConversationTranscriptQuery,
    ) -> Result<ConversationTranscriptPage, StorageError> {
        let after_sequence = query.after_sequence.unwrap_or(0);
        if after_sequence < 0 {
            return Err(StorageError::InvalidInput(
                "compaction transcript afterSequence must be non-negative".to_string(),
            ));
        }
        let limit = query
            .limit
            .unwrap_or(DEFAULT_COMPACTION_TRANSCRIPT_PAGE_LIMIT);
        if !(1..=MAX_COMPACTION_TRANSCRIPT_PAGE_LIMIT).contains(&limit) {
            return Err(StorageError::InvalidInput(format!(
                "compaction transcript limit must be between 1 and {MAX_COMPACTION_TRANSCRIPT_PAGE_LIMIT}"
            )));
        }

        let checkpoint = match query.compaction_id.as_deref() {
            Some(compaction_id) => {
                self.load_conversation_compaction(session_id, compaction_id)
                    .await?
            }
            None => self
                .load_latest_conversation_compaction(session_id)
                .await?
                .ok_or_else(|| {
                    StorageError::InvalidInput(
                        "session has no conversation compaction to read back".to_string(),
                    )
                })?,
        };
        let fetch_limit = i64::from(limit) + 1;
        let rows: Vec<StoredMessageRow> = sqlx::query_as(
            "SELECT id, turn_id, sequence, role, content, message_kind,
                    to_char(created_at, 'YYYY-MM-DD\"T\"HH24:MI:SS.US\"+08:00\"') AS created_at
             FROM messages
             WHERE session_id = $1
               AND sequence > $2
               AND sequence <= $3
             ORDER BY sequence
             LIMIT $4",
        )
        .bind(session_id.as_str())
        .bind(after_sequence)
        .bind(checkpoint.through_message_sequence)
        .bind(fetch_limit)
        .fetch_all(&self.pool)
        .await?;
        let has_more = rows.len() > limit as usize;
        let messages = rows
            .into_iter()
            .take(limit as usize)
            .map(
                |(id, turn_id, sequence, role, content, message_kind, created_at)| {
                    Ok(StoredMessageRecord {
                        id,
                        turn_id,
                        sequence,
                        role: parse_role(&role)?,
                        content: serde_json::from_value(content)?,
                        message_kind: parse_message_kind(&message_kind)?,
                        created_at,
                    })
                },
            )
            .collect::<Result<Vec<_>, StorageError>>()?;
        let next_after_sequence = has_more.then(|| {
            messages
                .last()
                .expect("a page with more rows has at least one returned message")
                .sequence
        });
        Ok(ConversationTranscriptPage {
            session_id: session_id.to_string(),
            compaction_id: checkpoint.id,
            through_message_sequence: checkpoint.through_message_sequence,
            messages,
            next_after_sequence,
            has_more,
        })
    }
    pub async fn load_latest_conversation_compaction(
        &self,
        session_id: &SessionId,
    ) -> Result<Option<ConversationCompaction>, StorageError> {
        let row: Option<ConversationCompactionRow> = sqlx::query_as(
            "SELECT id, session_id, sequence, through_message_sequence,
                    replaced_through_message_sequence, source_message_count,
                    checkpoint_format_version, kind, summary_format_version,
                    last_user_message_id, last_user_message_sequence,
                    resolved_model_name, summary, runtime_state,
                    runtime_reminder_format_version, runtime_reminder,
                    trigger_turn_id, parent_compaction_id, input_tokens, output_tokens,
                    to_char(created_at, 'YYYY-MM-DD\"T\"HH24:MI:SS.US\"+08:00\"') AS created_at
             FROM conversation_compactions
             WHERE session_id = $1
             ORDER BY sequence DESC
             LIMIT 1",
        )
        .bind(session_id.as_str())
        .fetch_optional(&self.pool)
        .await?;
        row.map(conversation_compaction_from_row).transpose()
    }

    pub async fn list_conversation_compactions(
        &self,
        session_id: &SessionId,
    ) -> Result<Vec<ConversationCompaction>, StorageError> {
        let rows: Vec<ConversationCompactionRow> = sqlx::query_as(
            "SELECT id, session_id, sequence, through_message_sequence,
                    replaced_through_message_sequence, source_message_count,
                    checkpoint_format_version, kind, summary_format_version,
                    last_user_message_id, last_user_message_sequence,
                    resolved_model_name, summary, runtime_state,
                    runtime_reminder_format_version, runtime_reminder,
                    trigger_turn_id, parent_compaction_id, input_tokens, output_tokens,
                    to_char(created_at, 'YYYY-MM-DD\"T\"HH24:MI:SS.US\"+08:00\"') AS created_at
             FROM conversation_compactions
             WHERE session_id = $1
             ORDER BY sequence DESC",
        )
        .bind(session_id.as_str())
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter()
            .map(conversation_compaction_from_row)
            .collect()
    }
    pub async fn load_conversation_compaction(
        &self,
        session_id: &SessionId,
        compaction_id: &str,
    ) -> Result<ConversationCompaction, StorageError> {
        let row: Option<ConversationCompactionRow> = sqlx::query_as(
            "SELECT id, session_id, sequence, through_message_sequence,
                    replaced_through_message_sequence, source_message_count,
                    checkpoint_format_version, kind, summary_format_version,
                    last_user_message_id, last_user_message_sequence,
                    resolved_model_name, summary, runtime_state,
                    runtime_reminder_format_version, runtime_reminder,
                    trigger_turn_id, parent_compaction_id, input_tokens, output_tokens,
                    to_char(created_at, 'YYYY-MM-DD\"T\"HH24:MI:SS.US\"+08:00\"') AS created_at
             FROM conversation_compactions
             WHERE session_id = $1 AND id = $2",
        )
        .bind(session_id.as_str())
        .bind(compaction_id)
        .fetch_optional(&self.pool)
        .await?;
        row.map(conversation_compaction_from_row)
            .transpose()?
            .ok_or_else(|| {
                StorageError::InvalidInput(format!(
                    "conversation compaction not found: {compaction_id}"
                ))
            })
    }
    /// Every Compaction Span in a Session, newest first.
    ///
    /// A manual compaction has no Turn, so its Span is unreachable through
    /// [`Self::get_trace`]. This is the read path for the Session scope the
    /// `compaction` kind was introduced with; it also returns the Turn-scoped
    /// threshold/overflow compactions so a Session's compaction history reads
    /// as one list.
    pub async fn list_compaction_spans(
        &self,
        session_id: &SessionId,
        limit: i64,
    ) -> Result<Vec<TraceSpanRecord>, StorageError> {
        let spans = sqlx::query_as::<_, TraceSpanRecord>(
            "SELECT id, trace_id, session_id, turn_id, parent_span_id, kind, name, status,
                    model_id, resolved_model_name, provider_request_id,
                    provider_call_id, requested_tool_name, resolved_tool_name,
                    attempt_count, input_tokens, output_tokens, cached_input_tokens,
                    reasoning_tokens, (input_tokens + output_tokens) AS total_tokens,
                    response_message_id, permission_wait_ms,
                    to_char(started_at,
                        'YYYY-MM-DD\"T\"HH24:MI:SS.US\"+08:00\"') AS started_at,
                    to_char(ended_at,
                        'YYYY-MM-DD\"T\"HH24:MI:SS.US\"+08:00\"') AS ended_at,
                    error_code, error_message, attributes
             FROM trace_spans
             WHERE session_id = $1 AND kind = 'compaction'
             ORDER BY started_at DESC, id
             LIMIT $2",
        )
        .bind(session_id.as_str())
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;
        Ok(spans)
    }
    pub(super) async fn save_conversation_compaction_inner(
        &self,
        session_id: &SessionId,
        input: NewConversationCompaction,
    ) -> Result<ConversationCompaction, StorageError> {
        if input.source_message_count == 0 {
            return Err(StorageError::InvalidInput(
                "compaction source message count must be positive".to_string(),
            ));
        }
        if input.resolved_model_name.trim().is_empty() {
            return Err(StorageError::InvalidInput(
                "compaction model name must not be blank".to_string(),
            ));
        }
        let summary = validate_summary_text(&input.summary)
            .map_err(|error| StorageError::InvalidInput(error.to_string()))?;
        validate_runtime_checkpoint_state(&input.runtime_state, &input.runtime_reminder)?;
        if input.kind == ConversationCompactionKind::Rewind {
            return Err(StorageError::InvalidInput(
                "rewind checkpoints must be created through the rewind operation".to_string(),
            ));
        }
        let trigger_turn_id = input.trigger_turn_id.as_ref().map(TurnId::as_str);
        let requires_trigger_turn = matches!(
            input.kind,
            ConversationCompactionKind::Threshold | ConversationCompactionKind::Overflow
        );
        if requires_trigger_turn != trigger_turn_id.is_some() {
            return Err(StorageError::InvalidInput(
                "threshold and overflow compactions require exactly one trigger turn".to_string(),
            ));
        }
        if input.last_user_message_id.is_some() != input.last_user_message_sequence.is_some() {
            return Err(StorageError::InvalidInput(
                "last user message id and sequence must be provided together".to_string(),
            ));
        }
        let source_message_count = i32::try_from(input.source_message_count).map_err(|_| {
            StorageError::InvalidInput("compaction source message count overflow".to_string())
        })?;
        let input_tokens = optional_token(input.input_tokens)?;
        let output_tokens = optional_token(input.output_tokens)?;
        let runtime_state = serde_json::to_value(&input.runtime_state)?;

        let mut transaction = self.pool.begin().await?;
        lock_session(&mut transaction, session_id).await?;
        match input.kind {
            ConversationCompactionKind::Manual => {
                let running_turn: bool = sqlx::query_scalar(
                    "SELECT EXISTS(
                         SELECT 1 FROM turns WHERE session_id = $1 AND status = 'running'
                     )",
                )
                .bind(session_id.as_str())
                .fetch_one(&mut *transaction)
                .await?;
                if running_turn {
                    return Err(StorageError::InvalidInput(format!(
                        "session has an active turn and cannot be compacted: {session_id}"
                    )));
                }
            }
            ConversationCompactionKind::Threshold | ConversationCompactionKind::Overflow => {
                let active_trigger: bool = sqlx::query_scalar(
                    "SELECT EXISTS(
                         SELECT 1 FROM turns
                         WHERE id = $1 AND session_id = $2 AND status = 'running'
                     )",
                )
                .bind(trigger_turn_id)
                .bind(session_id.as_str())
                .fetch_one(&mut *transaction)
                .await?;
                if !active_trigger {
                    return Err(StorageError::InvalidInput(
                        "threshold or overflow compaction trigger turn is not active in this session"
                            .to_string(),
                    ));
                }
            }
            ConversationCompactionKind::Rewind => unreachable!("validated above"),
        }
        let through_message_sequence: i64 = sqlx::query_scalar(
            "SELECT COALESCE(MAX(sequence), 0) FROM messages WHERE session_id = $1",
        )
        .bind(session_id.as_str())
        .fetch_one(&mut *transaction)
        .await?;
        if through_message_sequence == 0 {
            return Err(StorageError::InvalidInput(
                "conversation is empty and cannot be compacted".to_string(),
            ));
        }
        let (last_user_message_id, last_user_message_sequence) =
            match (input.last_user_message_id, input.last_user_message_sequence) {
                (Some(message_id), Some(message_sequence)) => {
                    let valid: bool = sqlx::query_scalar(
                        "SELECT EXISTS(
                             SELECT 1 FROM messages
                             WHERE id = $1 AND session_id = $2 AND sequence = $3
                               AND role = 'user' AND sequence <= $4
                         )",
                    )
                    .bind(&message_id)
                    .bind(session_id.as_str())
                    .bind(message_sequence)
                    .bind(through_message_sequence)
                    .fetch_one(&mut *transaction)
                    .await?;
                    if !valid {
                        return Err(StorageError::InvalidInput(
                            "compaction last user message does not belong to the source boundary"
                                .to_string(),
                        ));
                    }
                    (message_id, message_sequence)
                }
                (None, None) => sqlx::query_as::<_, (String, i64)>(
                    "SELECT id, sequence FROM messages
                         WHERE session_id = $1 AND role = 'user' AND sequence <= $2
                         ORDER BY sequence DESC LIMIT 1",
                )
                .bind(session_id.as_str())
                .bind(through_message_sequence)
                .fetch_optional(&mut *transaction)
                .await?
                .ok_or_else(|| {
                    StorageError::InvalidInput(
                        "conversation has no user request to preserve".to_string(),
                    )
                })?,
                _ => unreachable!("validated above"),
            };
        let sequence: i64 = sqlx::query_scalar(
            "SELECT COALESCE(MAX(sequence), 0) + 1
             FROM conversation_compactions WHERE session_id = $1",
        )
        .bind(session_id.as_str())
        .fetch_one(&mut *transaction)
        .await?;
        let id = format!("compaction-{}", Uuid::new_v4().simple());
        let row: ConversationCompactionRow = sqlx::query_as(
            "INSERT INTO conversation_compactions (
                 id, session_id, sequence, through_message_sequence,
                 replaced_through_message_sequence, source_message_count,
                 checkpoint_format_version, kind, summary_format_version,
                 last_user_message_id, last_user_message_sequence,
                 resolved_model_name, summary, runtime_state,
                 runtime_reminder_format_version, runtime_reminder,
                 trigger_turn_id, parent_compaction_id, input_tokens, output_tokens
             ) VALUES (
                 $1, $2, $3, $4, $4, $5, 1, $6, 1, $7, $8,
                 $9, $10, $11, 1, $12, $13, NULL, $14, $15
             )
             RETURNING id, session_id, sequence, through_message_sequence,
                       replaced_through_message_sequence, source_message_count,
                       checkpoint_format_version, kind, summary_format_version,
                       last_user_message_id, last_user_message_sequence,
                       resolved_model_name, summary, runtime_state,
                       runtime_reminder_format_version, runtime_reminder,
                       trigger_turn_id, parent_compaction_id, input_tokens, output_tokens,
                       to_char(created_at, 'YYYY-MM-DD\"T\"HH24:MI:SS.US\"+08:00\"') AS created_at",
        )
        .bind(&id)
        .bind(session_id.as_str())
        .bind(sequence)
        .bind(through_message_sequence)
        .bind(source_message_count)
        .bind(input.kind.as_str())
        .bind(&last_user_message_id)
        .bind(last_user_message_sequence)
        .bind(&input.resolved_model_name)
        .bind(&summary)
        .bind(runtime_state)
        .bind(&input.runtime_reminder)
        .bind(trigger_turn_id)
        .bind(input_tokens)
        .bind(output_tokens)
        .fetch_one(&mut *transaction)
        .await?;
        if let Some(turn_id) = trigger_turn_id {
            sqlx::query(
                "UPDATE turns SET
                     input_tokens = CASE WHEN $2::BIGINT IS NULL THEN input_tokens
                         ELSE COALESCE(input_tokens, 0) + $2 END,
                     output_tokens = CASE WHEN $3::BIGINT IS NULL THEN output_tokens
                         ELSE COALESCE(output_tokens, 0) + $3 END,
                     updated_at = CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai'
                 WHERE id = $1",
            )
            .bind(turn_id)
            .bind(input_tokens)
            .bind(output_tokens)
            .execute(&mut *transaction)
            .await?;
        }
        transaction.commit().await?;
        conversation_compaction_from_row(row)
    }

    pub(super) async fn rewind_conversation_compaction_inner(
        &self,
        session_id: &SessionId,
        compaction_id: &str,
        runtime_state: CompactionRuntimeState,
        runtime_reminder: String,
    ) -> Result<ConversationCompaction, StorageError> {
        validate_runtime_checkpoint_state(&runtime_state, &runtime_reminder)?;
        let runtime_state = serde_json::to_value(runtime_state)?;
        let mut transaction = self.pool.begin().await?;
        lock_session(&mut transaction, session_id).await?;
        let running_turn: bool = sqlx::query_scalar(
            "SELECT EXISTS(
                 SELECT 1 FROM turns WHERE session_id = $1 AND status = 'running'
             )",
        )
        .bind(session_id.as_str())
        .fetch_one(&mut *transaction)
        .await?;
        if running_turn {
            return Err(StorageError::InvalidInput(format!(
                "session has an active turn and cannot be rewound: {session_id}"
            )));
        }

        let source: Option<ConversationCompactionRow> = sqlx::query_as(
            "SELECT id, session_id, sequence, through_message_sequence,
                    replaced_through_message_sequence, source_message_count,
                    checkpoint_format_version, kind, summary_format_version,
                    last_user_message_id, last_user_message_sequence,
                    resolved_model_name, summary, runtime_state,
                    runtime_reminder_format_version, runtime_reminder,
                    trigger_turn_id, parent_compaction_id, input_tokens, output_tokens,
                    to_char(created_at, 'YYYY-MM-DD\"T\"HH24:MI:SS.US\"+08:00\"') AS created_at
             FROM conversation_compactions
             WHERE session_id = $1 AND id = $2
             FOR SHARE",
        )
        .bind(session_id.as_str())
        .bind(compaction_id)
        .fetch_optional(&mut *transaction)
        .await?;
        let source = source.ok_or_else(|| {
            StorageError::InvalidInput(format!(
                "conversation compaction not found: {compaction_id}"
            ))
        })?;
        let source_checkpoint_format =
            stored_format_version(source.checkpoint_format_version, "checkpoint")?;
        if source_checkpoint_format > 1 {
            return Err(StorageError::InvalidInput(format!(
                "unsupported stored checkpoint format version: {source_checkpoint_format}"
            )));
        }
        let source_summary_format =
            stored_format_version(source.summary_format_version, "summary")?;
        match source_summary_format {
            0 => {}
            1 => {
                validate_summary_text(&source.summary)
                    .map_err(|error| StorageError::InvalidInput(error.to_string()))?;
            }
            version => {
                return Err(StorageError::InvalidInput(format!(
                    "unsupported stored summary format version: {version}"
                )));
            }
        }
        if source.last_user_message_id.is_none() || source.last_user_message_sequence.is_none() {
            return Err(StorageError::InvalidInput(format!(
                "conversation compaction cannot be restored because it has no last user anchor: {compaction_id}"
            )));
        }

        let replaced_through_message_sequence: i64 = sqlx::query_scalar(
            "SELECT COALESCE(MAX(sequence), 0) FROM messages WHERE session_id = $1",
        )
        .bind(session_id.as_str())
        .fetch_one(&mut *transaction)
        .await?;
        let sequence: i64 = sqlx::query_scalar(
            "SELECT COALESCE(MAX(sequence), 0) + 1
             FROM conversation_compactions WHERE session_id = $1",
        )
        .bind(session_id.as_str())
        .fetch_one(&mut *transaction)
        .await?;
        let id = format!("compaction-{}", Uuid::new_v4().simple());
        let row: ConversationCompactionRow = sqlx::query_as(
            "INSERT INTO conversation_compactions (
                 id, session_id, sequence, through_message_sequence,
                 replaced_through_message_sequence, source_message_count,
                 checkpoint_format_version, kind, summary_format_version,
                 last_user_message_id, last_user_message_sequence,
                 resolved_model_name, summary, runtime_state,
                 runtime_reminder_format_version, runtime_reminder,
                 trigger_turn_id, parent_compaction_id, input_tokens, output_tokens
             ) VALUES (
                 $1, $2, $3, $4, $5, $6, $7, 'rewind', $8, $9, $10,
                 $11, $12, $13, $14, $15, NULL, $16, $17, $18
             )
             RETURNING id, session_id, sequence, through_message_sequence,
                       replaced_through_message_sequence, source_message_count,
                       checkpoint_format_version, kind, summary_format_version,
                       last_user_message_id, last_user_message_sequence,
                       resolved_model_name, summary, runtime_state,
                       runtime_reminder_format_version, runtime_reminder,
                       trigger_turn_id, parent_compaction_id, input_tokens, output_tokens,
                       to_char(created_at, 'YYYY-MM-DD\"T\"HH24:MI:SS.US\"+08:00\"') AS created_at",
        )
        .bind(&id)
        .bind(session_id.as_str())
        .bind(sequence)
        .bind(source.through_message_sequence)
        .bind(replaced_through_message_sequence)
        .bind(source.source_message_count)
        .bind(1_i16)
        .bind(source.summary_format_version)
        .bind(&source.last_user_message_id)
        .bind(source.last_user_message_sequence)
        .bind(&source.resolved_model_name)
        .bind(&source.summary)
        .bind(runtime_state)
        .bind(1_i16)
        .bind(runtime_reminder)
        .bind(&source.id)
        .bind(source.input_tokens)
        .bind(source.output_tokens)
        .fetch_one(&mut *transaction)
        .await?;
        transaction.commit().await?;
        conversation_compaction_from_row(row)
    }

    pub(super) async fn delete_conversation_compaction_inner(
        &self,
        session_id: &SessionId,
        compaction_id: &str,
    ) -> Result<(), StorageError> {
        let result =
            sqlx::query("DELETE FROM conversation_compactions WHERE id = $1 AND session_id = $2")
                .bind(compaction_id)
                .bind(session_id.as_str())
                .execute(&self.pool)
                .await?;
        if result.rows_affected() != 1 {
            return Err(StorageError::InvalidInput(format!(
                "conversation compaction not found: {compaction_id}"
            )));
        }
        Ok(())
    }
}
