use super::*;

impl PostgresStorage {
    pub async fn load_messages(
        &self,
        session_id: &SessionId,
    ) -> Result<Vec<Message>, StorageError> {
        Ok(self
            .load_conversation_records(session_id)
            .await?
            .into_iter()
            .map(|record| Message {
                role: record.role,
                content: record.content,
            })
            .collect())
    }

    pub async fn load_conversation_items(
        &self,
        session_id: &SessionId,
    ) -> Result<Vec<ConversationItem>, StorageError> {
        let compaction = self.load_latest_conversation_compaction(session_id).await?;
        let replaced_through = compaction
            .as_ref()
            .map_or(0, |record| record.replaced_through_message_sequence);
        let rows = self
            .load_message_records_between(session_id, replaced_through, None)
            .await?;
        let mut items = Vec::with_capacity(rows.len() + usize::from(compaction.is_some()) * 3);
        if let Some(compaction) = &compaction {
            let last_user = self.load_compaction_last_user(compaction).await?;
            items.extend(
                compacted_items(compaction, last_user)
                    .map_err(|error| StorageError::InvalidInput(error.to_string()))?,
            );
        }
        // The kind must survive the round trip: `last_real_user` distinguishes a
        // real user request from Skill bodies and sub-agent messages by kind, not
        // by role or position. Dropping it here silently reintroduces that bug.
        items.extend(rows.into_iter().map(|record| {
            ConversationItem::persisted_with_kind(
                record.id,
                record.sequence,
                record.message_kind,
                Message {
                    role: record.role,
                    content: record.content,
                },
            )
        }));
        Ok(items)
    }

    /// Loads the current model-visible Conversation projection. The raw
    /// transcript remains in `messages`; the latest compaction summary replaces
    /// only the prefix through its recorded message sequence.
    pub async fn load_conversation_records(
        &self,
        session_id: &SessionId,
    ) -> Result<Vec<StoredMessageRecord>, StorageError> {
        let compaction = self.load_latest_conversation_compaction(session_id).await?;
        let through_sequence = compaction
            .as_ref()
            .map_or(0, |record| record.replaced_through_message_sequence);
        let rows = self
            .load_message_records_between(session_id, through_sequence, None)
            .await?;
        let mut records = Vec::with_capacity(rows.len() + usize::from(compaction.is_some()) * 3);
        if let Some(compaction) = &compaction {
            let last_user = self.load_compaction_last_user_record(compaction).await?;
            records.push(StoredMessageRecord {
                id: format!("{}:last-user", compaction.id),
                turn_id: None,
                sequence: compaction.replaced_through_message_sequence,
                role: last_user.role,
                content: last_user.content,
                message_kind: MessageKind::Normal,
                created_at: compaction.created_at.clone(),
            });
            records.push(StoredMessageRecord {
                id: format!("{}:summary", compaction.id),
                turn_id: None,
                sequence: compaction.replaced_through_message_sequence,
                role: Role::User,
                content: compaction_summary_message(&compaction.summary).content,
                message_kind: MessageKind::Normal,
                created_at: compaction.created_at.clone(),
            });
            records.push(StoredMessageRecord {
                id: format!("{}:reminder", compaction.id),
                turn_id: None,
                sequence: compaction.replaced_through_message_sequence,
                role: Role::User,
                content: Message::text(Role::User, &compaction.runtime_reminder).content,
                message_kind: MessageKind::Normal,
                created_at: compaction.created_at.clone(),
            });
        }
        records.extend(rows);
        Ok(records)
    }

    pub async fn replay_conversation(
        &self,
        session_id: &SessionId,
        selector: ConversationProjectionSelector,
    ) -> Result<ConversationProjectionRecord, StorageError> {
        match &selector {
            ConversationProjectionSelector::Latest => {
                let checkpoint = self.load_latest_conversation_compaction(session_id).await?;
                let through_message_sequence: i64 = sqlx::query_scalar(
                    "SELECT COALESCE(MAX(sequence), 0) FROM messages WHERE session_id = $1",
                )
                .bind(session_id.as_str())
                .fetch_one(&self.pool)
                .await?;
                Ok(ConversationProjectionRecord {
                    selector: selector.clone(),
                    checkpoint_id: checkpoint.map(|checkpoint| checkpoint.id),
                    through_message_sequence,
                    messages: self.load_conversation_records(session_id).await?,
                })
            }
            ConversationProjectionSelector::Compaction { compaction_id } => {
                let checkpoint = self
                    .load_conversation_compaction(session_id, compaction_id)
                    .await?;
                Ok(ConversationProjectionRecord {
                    selector: selector.clone(),
                    checkpoint_id: Some(checkpoint.id.clone()),
                    through_message_sequence: checkpoint.through_message_sequence,
                    messages: self.compaction_projection_records(&checkpoint).await?,
                })
            }
            ConversationProjectionSelector::ThroughMessage { sequence } => {
                if *sequence < 0 {
                    return Err(StorageError::InvalidInput(
                        "conversation replay sequence must be non-negative".to_string(),
                    ));
                }
                let checkpoint = self
                    .load_latest_compaction_through(session_id, *sequence)
                    .await?;
                let mut messages = match &checkpoint {
                    Some(checkpoint) => self.compaction_projection_records(checkpoint).await?,
                    None => Vec::new(),
                };
                let after_sequence = checkpoint
                    .as_ref()
                    .map_or(0, |checkpoint| checkpoint.replaced_through_message_sequence);
                messages.extend(
                    self.load_message_records_between(session_id, after_sequence, Some(*sequence))
                        .await?,
                );
                Ok(ConversationProjectionRecord {
                    selector: selector.clone(),
                    checkpoint_id: checkpoint.map(|checkpoint| checkpoint.id),
                    through_message_sequence: *sequence,
                    messages,
                })
            }
        }
    }
    async fn compaction_projection_records(
        &self,
        compaction: &ConversationCompaction,
    ) -> Result<Vec<StoredMessageRecord>, StorageError> {
        let last_user = self.load_compaction_last_user_record(compaction).await?;
        Ok(vec![
            StoredMessageRecord {
                id: format!("{}:last-user", compaction.id),
                turn_id: None,
                sequence: compaction.replaced_through_message_sequence,
                role: last_user.role,
                content: last_user.content,
                message_kind: MessageKind::Normal,
                created_at: compaction.created_at.clone(),
            },
            StoredMessageRecord {
                id: format!("{}:summary", compaction.id),
                turn_id: None,
                sequence: compaction.replaced_through_message_sequence,
                role: Role::User,
                content: compaction_summary_message(&compaction.summary).content,
                message_kind: MessageKind::Normal,
                created_at: compaction.created_at.clone(),
            },
            StoredMessageRecord {
                id: format!("{}:reminder", compaction.id),
                turn_id: None,
                sequence: compaction.replaced_through_message_sequence,
                role: Role::User,
                content: Message::text(Role::User, &compaction.runtime_reminder).content,
                message_kind: MessageKind::Normal,
                created_at: compaction.created_at.clone(),
            },
        ])
    }

    pub(super) async fn load_compaction_last_user(
        &self,
        compaction: &ConversationCompaction,
    ) -> Result<Message, StorageError> {
        let record = self.load_compaction_last_user_record(compaction).await?;
        Ok(Message {
            role: record.role,
            content: record.content,
        })
    }

    async fn load_compaction_last_user_record(
        &self,
        compaction: &ConversationCompaction,
    ) -> Result<StoredMessageRecord, StorageError> {
        let message_id = compaction.last_user_message_id.as_deref().ok_or_else(|| {
            StorageError::InvalidInput(format!(
                "compaction {} has no last user message",
                compaction.id
            ))
        })?;
        let row: Option<StoredMessageRow> = sqlx::query_as(
            "SELECT id, turn_id, sequence, role, content, message_kind,
                    to_char(created_at, 'YYYY-MM-DD\"T\"HH24:MI:SS.US\"+08:00\"') AS created_at
             FROM messages
             WHERE session_id = $1 AND id = $2 AND role = 'user'",
        )
        .bind(compaction.session_id.as_str())
        .bind(message_id)
        .fetch_optional(&self.pool)
        .await?;
        let (id, turn_id, sequence, role, content, message_kind, created_at) =
            row.ok_or_else(|| {
                StorageError::InvalidInput(format!(
                    "compaction {} last user message was not found",
                    compaction.id
                ))
            })?;
        Ok(StoredMessageRecord {
            id,
            turn_id,
            sequence,
            role: parse_role(&role)?,
            content: serde_json::from_value(content)?,
            message_kind: parse_message_kind(&message_kind)?,
            created_at,
        })
    }

    async fn load_message_records_between(
        &self,
        session_id: &SessionId,
        after_sequence: i64,
        through_sequence: Option<i64>,
    ) -> Result<Vec<StoredMessageRecord>, StorageError> {
        let rows: Vec<StoredMessageRow> = sqlx::query_as(
            "SELECT id, turn_id, sequence, role, content, message_kind,
                    to_char(created_at, 'YYYY-MM-DD\"T\"HH24:MI:SS.US\"+08:00\"') AS created_at
             FROM messages
             WHERE session_id = $1
               AND sequence > $2
               AND ($3::BIGINT IS NULL OR sequence <= $3)
             ORDER BY sequence",
        )
        .bind(session_id.as_str())
        .bind(after_sequence)
        .bind(through_sequence)
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter()
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
            .collect()
    }

    pub async fn load_message_records(
        &self,
        session_id: &SessionId,
    ) -> Result<Vec<StoredMessageRecord>, StorageError> {
        let rows: Vec<StoredMessageRow> = sqlx::query_as(
            "SELECT id, turn_id, sequence, role, content, message_kind,
                    to_char(created_at, 'YYYY-MM-DD\"T\"HH24:MI:SS.US\"+08:00\"') AS created_at
             FROM messages
             WHERE session_id = $1
             ORDER BY sequence",
        )
        .bind(session_id.as_str())
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter()
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
            .collect()
    }
    async fn load_latest_compaction_through(
        &self,
        session_id: &SessionId,
        through_message_sequence: i64,
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
             WHERE session_id = $1 AND replaced_through_message_sequence <= $2
             ORDER BY replaced_through_message_sequence DESC, sequence DESC
             LIMIT 1",
        )
        .bind(session_id.as_str())
        .bind(through_message_sequence)
        .fetch_optional(&self.pool)
        .await?;
        row.map(conversation_compaction_from_row).transpose()
    }
    pub async fn replace_message_contents(
        &self,
        session_id: &SessionId,
        updates: &[(String, Vec<ContentBlock>)],
    ) -> Result<(), StorageError> {
        if updates.is_empty() {
            return Ok(());
        }
        let mut transaction = self.pool.begin().await?;
        lock_session(&mut transaction, session_id).await?;
        for (message_id, content) in updates {
            let encoded = serde_json::to_value(content)?;
            let result = sqlx::query(
                "UPDATE messages
                 SET content = $3
                 WHERE id = $1 AND session_id = $2 AND role = 'tool'",
            )
            .bind(message_id)
            .bind(session_id.as_str())
            .bind(encoded)
            .execute(&mut *transaction)
            .await?;
            if result.rows_affected() != 1 {
                return Err(StorageError::InvalidInput(format!(
                    "tool message not found for content update: {message_id}"
                )));
            }
        }
        transaction.commit().await?;
        Ok(())
    }
}
