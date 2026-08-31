use super::*;

pub(super) type StoredMessageRow = (String, Option<String>, i64, String, Value, String, String);

pub(super) fn is_payload_reference_conflict(error: &sqlx::Error) -> bool {
    let sqlx::Error::Database(error) = error else {
        return false;
    };
    error.code().as_deref() == Some("23503")
        && error.constraint() == Some("trace_span_payloads_payload_hash_fkey")
}

pub(super) fn saturating_u32(value: usize) -> u32 {
    u32::try_from(value).unwrap_or(u32::MAX)
}
pub(super) async fn lock_session(
    transaction: &mut Transaction<'_, Postgres>,
    session_id: &SessionId,
) -> Result<(), StorageError> {
    let exists: Option<String> =
        sqlx::query_scalar("SELECT id FROM sessions WHERE id = $1 FOR UPDATE")
            .bind(session_id.as_str())
            .fetch_optional(&mut **transaction)
            .await?;
    if exists.is_none() {
        return Err(StorageError::SessionNotFound(session_id.to_string()));
    }
    Ok(())
}

pub(super) async fn lock_trace_payload_mutations(
    transaction: &mut Transaction<'_, Postgres>,
) -> Result<(), StorageError> {
    sqlx::query("SELECT pg_advisory_xact_lock($1)")
        .bind(TRACE_PAYLOAD_ADVISORY_LOCK)
        .execute(&mut **transaction)
        .await?;
    Ok(())
}

pub(super) async fn cleanup_trace_payload_candidates(
    transaction: &mut Transaction<'_, Postgres>,
    mut payload_hashes: Vec<String>,
) -> Result<(), StorageError> {
    payload_hashes.sort_unstable();
    payload_hashes.dedup();
    for payload_hash in payload_hashes {
        // Keep each candidate in a savepoint. RESTRICT is the final defense for
        // an already-committed reference written outside the shared lock path;
        // one such reference must not roll back cleanup of unrelated bodies.
        let mut cleanup = transaction.begin().await?;
        let deletion = sqlx::query(
            "DELETE FROM trace_payloads AS payload
             WHERE payload.hash = $1
               AND NOT EXISTS (
                   SELECT 1 FROM trace_span_payloads AS mapping
                   WHERE mapping.payload_hash = payload.hash
               )",
        )
        .bind(payload_hash)
        .execute(&mut *cleanup)
        .await;
        match deletion {
            Ok(_) => cleanup.commit().await?,
            Err(error) if is_payload_reference_conflict(&error) => cleanup.rollback().await?,
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}

pub(super) async fn lock_turn(
    transaction: &mut Transaction<'_, Postgres>,
    turn_id: &TurnId,
) -> Result<SessionId, StorageError> {
    let session_id: Option<String> =
        sqlx::query_scalar("SELECT session_id FROM turns WHERE id = $1 FOR UPDATE")
            .bind(turn_id.as_str())
            .fetch_optional(&mut **transaction)
            .await?;
    session_id
        .map(SessionId::new)
        .ok_or_else(|| StorageError::TurnNotFound(turn_id.to_string()))
}

pub(super) async fn next_turn_sequence(
    transaction: &mut Transaction<'_, Postgres>,
    session_id: &SessionId,
) -> Result<i64, StorageError> {
    let sequence: i64 = sqlx::query_scalar(
        "SELECT COALESCE(MAX(sequence), 0) + 1 FROM turns WHERE session_id = $1",
    )
    .bind(session_id.as_str())
    .fetch_one(&mut **transaction)
    .await?;
    Ok(sequence)
}

pub(super) async fn insert_message(
    transaction: &mut Transaction<'_, Postgres>,
    session_id: &SessionId,
    turn_id: Option<&TurnId>,
    role: Role,
    content: Value,
    message_kind: MessageKind,
    tool: Option<(&str, &str)>,
) -> Result<String, StorageError> {
    let (provider_call_id, tool_name) = tool
        .map(|(provider_call_id, tool_name)| (Some(provider_call_id), Some(tool_name)))
        .unwrap_or((None, None));
    let sequence: i64 = sqlx::query_scalar(
        "SELECT COALESCE(MAX(sequence), 0) + 1 FROM messages WHERE session_id = $1",
    )
    .bind(session_id.as_str())
    .fetch_one(&mut **transaction)
    .await?;
    let message_id = format!("msg-{}", Uuid::new_v4().simple());
    sqlx::query(
        "INSERT INTO messages (
             id, session_id, turn_id, sequence, role, content, message_kind,
             provider_call_id, tool_name
         ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)",
    )
    .bind(&message_id)
    .bind(session_id.as_str())
    .bind(turn_id.map(TurnId::as_str))
    .bind(sequence)
    .bind(role.as_provider_str())
    .bind(content)
    .bind(message_kind.as_str())
    .bind(provider_call_id)
    .bind(tool_name)
    .execute(&mut **transaction)
    .await?;
    Ok(message_id)
}
pub(super) fn optional_token(value: Option<u64>) -> Result<Option<i64>, StorageError> {
    value
        .map(|value| {
            i64::try_from(value)
                .map_err(|_| StorageError::InvalidInput("token count overflow".to_string()))
        })
        .transpose()
}
pub(super) fn stored_token(value: Option<i64>) -> Result<Option<u64>, StorageError> {
    value
        .map(|value| {
            u64::try_from(value).map_err(|_| {
                StorageError::InvalidInput("stored token count is negative".to_string())
            })
        })
        .transpose()
}

pub(super) fn conversation_compaction_from_row(
    row: ConversationCompactionRow,
) -> Result<ConversationCompaction, StorageError> {
    let kind = match row.kind.as_str() {
        "manual" => ConversationCompactionKind::Manual,
        "threshold" => ConversationCompactionKind::Threshold,
        "overflow" => ConversationCompactionKind::Overflow,
        "rewind" => ConversationCompactionKind::Rewind,
        other => {
            return Err(StorageError::InvalidInput(format!(
                "unknown stored compaction kind: {other}"
            )));
        }
    };
    let checkpoint_format_version =
        stored_format_version(row.checkpoint_format_version, "checkpoint")?;
    if checkpoint_format_version > 1 {
        return Err(StorageError::InvalidInput(format!(
            "unsupported stored checkpoint format version: {checkpoint_format_version}"
        )));
    }
    let summary_format_version = stored_format_version(row.summary_format_version, "summary")?;
    if summary_format_version > 1 {
        return Err(StorageError::InvalidInput(format!(
            "unsupported stored summary format version: {summary_format_version}"
        )));
    }
    let summary = if summary_format_version == 1 {
        validate_summary_text(&row.summary)
            .map_err(|error| StorageError::InvalidInput(error.to_string()))?
    } else {
        row.summary
    };
    let runtime_reminder_format_version =
        stored_format_version(row.runtime_reminder_format_version, "runtime reminder")?;
    if runtime_reminder_format_version != 1 {
        return Err(StorageError::InvalidInput(format!(
            "unsupported stored runtime reminder format version: {runtime_reminder_format_version}"
        )));
    }
    let runtime_state: CompactionRuntimeState = serde_json::from_value(row.runtime_state)?;
    validate_runtime_checkpoint_state(&runtime_state, &row.runtime_reminder)?;

    Ok(ConversationCompaction {
        id: row.id,
        session_id: row.session_id,
        sequence: row.sequence,
        through_message_sequence: row.through_message_sequence,
        replaced_through_message_sequence: row.replaced_through_message_sequence,
        source_message_count: u32::try_from(row.source_message_count).map_err(|_| {
            StorageError::InvalidInput(
                "stored compaction source message count is invalid".to_string(),
            )
        })?,
        checkpoint_format_version,
        kind,
        summary_format_version,
        last_user_message_id: row.last_user_message_id,
        last_user_message_sequence: row.last_user_message_sequence,
        resolved_model_name: row.resolved_model_name,
        summary,
        runtime_state,
        runtime_reminder_format_version,
        runtime_reminder: row.runtime_reminder,
        trigger_turn_id: row.trigger_turn_id,
        parent_compaction_id: row.parent_compaction_id,
        input_tokens: stored_token(row.input_tokens)?,
        output_tokens: stored_token(row.output_tokens)?,
        created_at: row.created_at,
    })
}

pub(super) fn stored_format_version(value: i16, name: &str) -> Result<u16, StorageError> {
    u16::try_from(value).map_err(|_| {
        StorageError::InvalidInput(format!("stored {name} format version is negative"))
    })
}

pub(super) fn parse_role(value: &str) -> Result<Role, StorageError> {
    match value {
        "system" => Ok(Role::System),
        "user" => Ok(Role::User),
        "assistant" => Ok(Role::Assistant),
        "tool" => Ok(Role::Tool),
        other => Err(StorageError::InvalidInput(format!(
            "unknown stored message role: {other}"
        ))),
    }
}

pub(super) fn parse_message_kind(value: &str) -> Result<MessageKind, StorageError> {
    match value {
        "normal" => Ok(MessageKind::Normal),
        "skill_instruction" => Ok(MessageKind::SkillInstruction),
        "agent_message" => Ok(MessageKind::AgentMessage),
        "world_state" => Ok(MessageKind::WorldState),
        other => Err(StorageError::InvalidInput(format!(
            "unknown stored message kind: {other}"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 出库解析必须认得每一个入库写得出的取值。
    ///
    /// 解析不认得的后果不是丢一条消息，而是整个会话加载失败——`parse_message_kind`
    /// 返回 `Err` 会让读取该 Session 的调用整体报错。
    #[test]
    fn every_written_kind_can_be_read_back() {
        for kind in [
            MessageKind::Normal,
            MessageKind::SkillInstruction,
            MessageKind::AgentMessage,
            MessageKind::WorldState,
        ] {
            assert_eq!(
                parse_message_kind(kind.as_str()).expect("stored kind must parse"),
                kind
            );
        }
    }

    /// 未知取值仍然要明确失败，不能默默当成 Normal。
    ///
    /// 静默降级会把一条 world-state 消息变成用户请求，压缩时被 replay 出去。
    #[test]
    fn an_unknown_kind_is_rejected_rather_than_defaulted() {
        assert!(parse_message_kind("not_a_kind").is_err());
    }
}
