use super::*;

const SESSION_COLUMNS: &str = "SELECT id, title, working_directory, default_model_id, status,
            to_char(created_at, 'YYYY-MM-DD\"T\"HH24:MI:SS.US\"+08:00\"') AS created_at,
            to_char(updated_at, 'YYYY-MM-DD\"T\"HH24:MI:SS.US\"+08:00\"') AS updated_at,
            to_char(last_turn_at, 'YYYY-MM-DD\"T\"HH24:MI:SS.US\"+08:00\"') AS last_turn_at,
            parent_session_id, task_name, agent_role, spawn_span_id
     FROM sessions";

impl PostgresStorage {
    pub async fn create_session(
        &self,
        input: &SessionInput,
    ) -> Result<SessionRecord, StorageError> {
        validate_session(input)?;
        sqlx::query(
            "INSERT INTO sessions (id, title, working_directory, default_model_id)
             VALUES ($1, $2, $3, $4)",
        )
        .bind(input.id.as_str())
        .bind(&input.title)
        .bind(&input.working_directory)
        .bind(&input.default_model_id)
        .execute(&self.pool)
        .await?;
        self.load_session(&input.id)
            .await?
            .ok_or_else(|| StorageError::SessionNotFound(input.id.to_string()))
    }

    /// Creates a sub-agent Session under `parent_session_id`.
    ///
    /// The parent must exist and must itself be a root Session: nesting depth is
    /// capped at one. The database also carries `sessions_spawn_not_self`, but
    /// that only blocks a direct self-loop — the real depth rule lives here, on
    /// the single insert path.
    pub async fn create_sub_agent_session(
        &self,
        input: &SubAgentSessionInput,
    ) -> Result<SessionRecord, StorageError> {
        validate_sub_agent_session(input)?;

        let parent = self
            .load_session(&input.parent_session_id)
            .await?
            .ok_or_else(|| StorageError::SessionNotFound(input.parent_session_id.to_string()))?;
        if parent.is_sub_agent() {
            return Err(StorageError::InvalidInput(format!(
                "session {} is already a sub-agent; nesting depth is capped at one",
                input.parent_session_id
            )));
        }

        sqlx::query(
            "INSERT INTO sessions
                 (id, working_directory, default_model_id,
                  parent_session_id, task_name, agent_role, spawn_span_id)
             VALUES ($1, $2, $3, $4, $5, $6, $7)",
        )
        .bind(input.id.as_str())
        .bind(&input.working_directory)
        .bind(&input.default_model_id)
        .bind(input.parent_session_id.as_str())
        .bind(&input.task_name)
        .bind(&input.agent_role)
        .bind(&input.spawn_span_id)
        .execute(&self.pool)
        .await?;

        self.load_session(&input.id)
            .await?
            .ok_or_else(|| StorageError::SessionNotFound(input.id.to_string()))
    }

    /// Lists top-level Sessions only. Sub-agents are reachable through their
    /// parent, never through the session list.
    pub async fn list_sessions(&self) -> Result<Vec<SessionRecord>, StorageError> {
        let query = format!(
            "{SESSION_COLUMNS} WHERE parent_session_id IS NULL ORDER BY updated_at DESC, id"
        );
        let sessions = sqlx::query_as::<_, SessionRecord>(&query)
            .fetch_all(&self.pool)
            .await?;
        Ok(sessions)
    }

    /// Lists the direct sub-agents of `parent_session_id`, oldest first.
    pub async fn list_sub_agent_sessions(
        &self,
        parent_session_id: &SessionId,
    ) -> Result<Vec<SessionRecord>, StorageError> {
        let query =
            format!("{SESSION_COLUMNS} WHERE parent_session_id = $1 ORDER BY created_at, id");
        let sessions = sqlx::query_as::<_, SessionRecord>(&query)
            .bind(parent_session_id.as_str())
            .fetch_all(&self.pool)
            .await?;
        Ok(sessions)
    }

    /// Finds terminal child turns whose deterministic parent message is absent,
    /// and removes child Sessions that never started a Turn.
    ///
    /// The parent Session row is locked so a concurrent spawn cannot race the
    /// zero-Turn orphan deletion. This method only prepares mailbox deliveries;
    /// it never starts a parent Turn.
    pub async fn reconcile_sub_agent_sessions(
        &self,
        parent_session_id: &SessionId,
    ) -> Result<SubAgentReconciliation, StorageError> {
        let mut transaction = self.pool.begin().await?;
        lock_session(&mut transaction, parent_session_id).await?;

        let deleted_orphans = sqlx::query_as::<_, DeletedOrphanSubAgent>(
            "DELETE FROM sessions AS child
             WHERE child.parent_session_id = $1
               AND NOT EXISTS (
                   SELECT 1 FROM turns AS turn WHERE turn.session_id = child.id
               )
             RETURNING child.id AS session_id, child.task_name",
        )
        .bind(parent_session_id.as_str())
        .fetch_all(&mut *transaction)
        .await?;

        let rows = sqlx::query_as::<_, UndeliveredSubAgentResultRow>(
            "SELECT child.id AS child_session_id,
                    turn.id AS child_turn_id,
                    child.task_name,
                    turn.status,
                    turn.error_code,
                    turn.error_message,
                    assistant.content AS assistant_content
             FROM sessions AS child
             JOIN turns AS turn ON turn.session_id = child.id
             LEFT JOIN LATERAL (
                 SELECT message.content
                 FROM messages AS message
                 WHERE message.session_id = child.id
                   AND message.turn_id = turn.id
                   AND message.role = 'assistant'
                 ORDER BY message.sequence DESC
                 LIMIT 1
             ) AS assistant ON TRUE
             WHERE child.parent_session_id = $1
               AND turn.status IN ('completed', 'failed', 'cancelled', 'interrupted')
               AND NOT EXISTS (
                   SELECT 1
                   FROM messages AS delivered
                   WHERE delivered.session_id = $1
                     AND delivered.id = CONCAT(
                         'agent-msg:', child.id, ':', turn.id, ':',
                         CASE
                             WHEN turn.status = 'completed' THEN 'final_answer'
                             WHEN turn.status = 'interrupted' THEN 'interrupted'
                             ELSE 'failed'
                         END
                     )
               )
             ORDER BY child.created_at, child.id, turn.sequence",
        )
        .bind(parent_session_id.as_str())
        .fetch_all(&mut *transaction)
        .await?;

        let mut undelivered = Vec::with_capacity(rows.len());
        for row in rows {
            let final_text = row
                .assistant_content
                .map(serde_json::from_value::<Vec<ContentBlock>>)
                .transpose()?
                .map(|content| {
                    content
                        .into_iter()
                        .filter_map(|block| match block {
                            ContentBlock::Text(text) => Some(text.text),
                            _ => None,
                        })
                        .collect::<Vec<_>>()
                        .join("\n")
                })
                .filter(|text| !text.is_empty());
            undelivered.push(UndeliveredSubAgentResult {
                child_session_id: SessionId::new(row.child_session_id),
                child_turn_id: TurnId::new(row.child_turn_id),
                task_name: row.task_name,
                status: row.status,
                error_code: row.error_code,
                error_message: row.error_message,
                final_text,
            });
        }

        transaction.commit().await?;
        Ok(SubAgentReconciliation {
            undelivered,
            deleted_orphans,
        })
    }

    pub async fn load_session(
        &self,
        session_id: &SessionId,
    ) -> Result<Option<SessionRecord>, StorageError> {
        let query = format!("{SESSION_COLUMNS} WHERE id = $1");
        let session = sqlx::query_as::<_, SessionRecord>(&query)
            .bind(session_id.as_str())
            .fetch_optional(&self.pool)
            .await?;
        Ok(session)
    }

    pub async fn rename_session(
        &self,
        session_id: &SessionId,
        title: &str,
    ) -> Result<SessionRecord, StorageError> {
        if title.trim().is_empty() {
            return Err(StorageError::InvalidInput(
                "session title must not be blank".to_string(),
            ));
        }
        let result = sqlx::query(
            "UPDATE sessions
                 SET title = $2, updated_at = CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai'
                 WHERE id = $1",
        )
        .bind(session_id.as_str())
        .bind(title.trim())
        .execute(&self.pool)
        .await?;
        if result.rows_affected() == 0 {
            return Err(StorageError::SessionNotFound(session_id.to_string()));
        }
        self.load_session(session_id)
            .await?
            .ok_or_else(|| StorageError::SessionNotFound(session_id.to_string()))
    }

    pub async fn delete_session(&self, session_id: &SessionId) -> Result<(), StorageError> {
        let mut transaction = self.pool.begin().await?;
        lock_trace_payload_mutations(&mut transaction).await?;
        // Block both new Span inserts (through the Session FK) and late finish
        // signals that could attach a payload to an already-created Span. The
        // candidate query must run only after in-flight Trace writes commit;
        // otherwise the Session cascade could leave their payload body orphaned.
        lock_session(&mut transaction, session_id).await?;
        let _: Vec<String> = sqlx::query_scalar(
            "SELECT id
             FROM sessions
             WHERE parent_session_id = $1
             FOR UPDATE",
        )
        .bind(session_id.as_str())
        .fetch_all(&mut *transaction)
        .await?;
        let _: Vec<String> = sqlx::query_scalar(
            "SELECT id
             FROM trace_spans
             WHERE session_id = $1
                OR session_id IN (
                    SELECT id FROM sessions WHERE parent_session_id = $1
                )
             FOR UPDATE",
        )
        .bind(session_id.as_str())
        .fetch_all(&mut *transaction)
        .await?;
        let payload_hashes = sqlx::query_scalar::<_, String>(
            "SELECT DISTINCT mapping.payload_hash
             FROM trace_span_payloads AS mapping
             JOIN trace_spans AS span ON span.id = mapping.span_id
             WHERE span.session_id = $1
                OR span.session_id IN (
                    SELECT id FROM sessions WHERE parent_session_id = $1
                )",
        )
        .bind(session_id.as_str())
        .fetch_all(&mut *transaction)
        .await?;
        let result = sqlx::query("DELETE FROM sessions WHERE id = $1")
            .bind(session_id.as_str())
            .execute(&mut *transaction)
            .await?;
        if result.rows_affected() == 0 {
            return Err(StorageError::SessionNotFound(session_id.to_string()));
        }
        cleanup_trace_payload_candidates(&mut transaction, payload_hashes).await?;
        transaction.commit().await?;
        Ok(())
    }

    /// Remove payload mappings older than the configured retention window.
    ///
    /// Age is measured from the owning Span, not `trace_payloads.created_at`:
    /// payload bodies are deduplicated, so a freshly written Span may point at
    /// a body row first created months earlier.
    pub async fn purge_expired_trace_payloads(
        &self,
        retention_days: u32,
    ) -> Result<usize, StorageError> {
        let retention_days = i32::try_from(retention_days).map_err(|_| {
            StorageError::InvalidInput(
                "trace payload retention must be between 1 and 2147483647 days".to_string(),
            )
        })?;
        if retention_days == 0 {
            return Err(StorageError::InvalidInput(
                "trace payload retention must be between 1 and 2147483647 days".to_string(),
            ));
        }

        let mut transaction = self.pool.begin().await?;
        lock_trace_payload_mutations(&mut transaction).await?;
        let payload_hashes = sqlx::query_scalar::<_, String>(
            "DELETE FROM trace_span_payloads AS mapping
             USING trace_spans AS span
             WHERE span.id = mapping.span_id
               AND span.started_at <
                   (CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai')
                   - make_interval(days => $1)
               AND NOT EXISTS (
                   SELECT 1
                   FROM trace_annotations AS annotation
                   WHERE annotation.session_id = span.session_id
                     AND annotation.trace_id = span.trace_id
               )
             RETURNING mapping.payload_hash",
        )
        .bind(retention_days)
        .fetch_all(&mut *transaction)
        .await?;
        let deleted_mapping_count = payload_hashes.len();
        cleanup_trace_payload_candidates(&mut transaction, payload_hashes).await?;
        transaction.commit().await?;
        Ok(deleted_mapping_count)
    }
    pub async fn mark_running_interrupted(&self) -> Result<u64, StorageError> {
        let mut transaction = self.pool.begin().await?;
        sqlx::query(
            "UPDATE trace_spans
             SET status = 'outcome_unknown',
                 ended_at = CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai',
                 error_code = COALESCE(error_code, 'process_restart'),
                 error_message = COALESCE(error_message, 'process restarted before span completed')
             WHERE status = 'running'",
        )
        .execute(&mut *transaction)
        .await?;
        let result = sqlx::query(
            "UPDATE turns
             SET status = 'interrupted',
                 ended_at = CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai',
                 updated_at = CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai',
                 error_code = COALESCE(error_code, 'process_restart'),
                 error_message = COALESCE(error_message, 'process restarted before turn completed')
             WHERE status = 'running'",
        )
        .execute(&mut *transaction)
        .await?;
        transaction.commit().await?;
        Ok(result.rows_affected())
    }
}
