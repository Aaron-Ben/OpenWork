use super::*;

const SESSION_COLUMNS: &str = "SELECT id, title, working_directory, default_model_id, status,
            to_char(created_at, 'YYYY-MM-DD\"T\"HH24:MI:SS.US\"+08:00\"') AS created_at,
            to_char(updated_at, 'YYYY-MM-DD\"T\"HH24:MI:SS.US\"+08:00\"') AS updated_at,
            to_char(last_turn_at, 'YYYY-MM-DD\"T\"HH24:MI:SS.US\"+08:00\"') AS last_turn_at
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

    pub async fn list_sessions(&self) -> Result<Vec<SessionRecord>, StorageError> {
        let query = format!("{SESSION_COLUMNS} ORDER BY updated_at DESC, id");
        let sessions = sqlx::query_as::<_, SessionRecord>(&query)
            .fetch_all(&self.pool)
            .await?;
        Ok(sessions)
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
        let _: Vec<String> =
            sqlx::query_scalar("SELECT id FROM trace_spans WHERE session_id = $1 FOR UPDATE")
                .bind(session_id.as_str())
                .fetch_all(&mut *transaction)
                .await?;
        let payload_hashes = sqlx::query_scalar::<_, String>(
            "SELECT DISTINCT mapping.payload_hash
             FROM trace_span_payloads AS mapping
             JOIN trace_spans AS span ON span.id = mapping.span_id
             WHERE span.session_id = $1",
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
