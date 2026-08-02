use super::*;

impl PostgresStorage {
    pub async fn list_traces(
        &self,
        session_id: Option<&SessionId>,
        limit: i64,
    ) -> Result<Vec<TraceTurnSummary>, StorageError> {
        if !(1..=500).contains(&limit) {
            return Err(StorageError::InvalidInput(
                "trace list limit must be between 1 and 500".to_string(),
            ));
        }
        // 两路来源：有 Turn 支撑的 Trace，以及无 Turn 的独立 Trace（手动压缩、rewind）。
        // 后者没有 Turn 行可以 JOIN，因此必须单独一路——只从 turns 出发会让它们永远不可见。
        let traces = sqlx::query_as::<_, TraceTurnSummary>(
            "SELECT turns.id AS trace_id, turns.id AS turn_id, turns.session_id,
                    turns.sequence AS turn_sequence, turns.status,
                    turns.resolved_model_name, turns.model_call_count,
                    turns.model_submission_count,
                    turns.tool_call_count, COUNT(spans.id)::BIGINT AS span_count,
                    COALESCE(SUM(spans.input_tokens + spans.output_tokens), 0)::BIGINT AS total_tokens,
                    to_char(turns.started_at,
                        'YYYY-MM-DD\"T\"HH24:MI:SS.US\"+08:00\"') AS started_at,
                    to_char(turns.ended_at,
                        'YYYY-MM-DD\"T\"HH24:MI:SS.US\"+08:00\"') AS ended_at,
                    turns.started_at AS sort_key
             FROM turns turns
             LEFT JOIN trace_spans spans ON spans.trace_id = turns.id
             WHERE ($1::TEXT IS NULL OR turns.session_id = $1)
             GROUP BY turns.id

             UNION ALL

             SELECT root.trace_id, NULL AS turn_id, root.session_id,
                    NULL AS turn_sequence,
                    -- Span 与 Turn 的状态词汇不同，映射到 Turn 的取值，
                    -- 让列表只有一套状态语言（前端过滤器与徽章依赖这一点）。
                    CASE root.status
                        WHEN 'succeeded' THEN 'completed'
                        WHEN 'outcome_unknown' THEN 'interrupted'
                        ELSE root.status
                    END AS status,
                    COALESCE(root.resolved_model_name, '') AS resolved_model_name,
                    0 AS model_call_count, 0 AS model_submission_count,
                    0 AS tool_call_count,
                    (SELECT COUNT(*)::BIGINT FROM trace_spans member
                     WHERE member.trace_id = root.trace_id) AS span_count,
                    (SELECT COALESCE(SUM(member.input_tokens + member.output_tokens), 0)::BIGINT
                     FROM trace_spans member
                     WHERE member.trace_id = root.trace_id) AS total_tokens,
                    to_char(root.started_at,
                        'YYYY-MM-DD\"T\"HH24:MI:SS.US\"+08:00\"') AS started_at,
                    to_char(root.ended_at,
                        'YYYY-MM-DD\"T\"HH24:MI:SS.US\"+08:00\"') AS ended_at,
                    root.started_at AS sort_key
             FROM trace_spans root
             WHERE root.turn_id IS NULL
               AND root.parent_span_id IS NULL
               AND ($1::TEXT IS NULL OR root.session_id = $1)

             ORDER BY sort_key DESC, trace_id
             LIMIT $2",
        )
        .bind(session_id.map(SessionId::as_str))
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;
        Ok(traces)
    }

    pub async fn get_trace(&self, turn_id: &TurnId) -> Result<TurnTrace, StorageError> {
        let mut summary = sqlx::query_as::<_, TraceTurnSummary>(
            "SELECT turns.id AS trace_id, turns.id AS turn_id, turns.session_id,
                    turns.sequence AS turn_sequence, turns.status,
                    turns.resolved_model_name, turns.model_call_count,
                    turns.model_submission_count,
                    turns.tool_call_count, COUNT(spans.id)::BIGINT AS span_count,
                    COALESCE(SUM(spans.input_tokens + spans.output_tokens), 0)::BIGINT AS total_tokens,
                    to_char(turns.started_at,
                        'YYYY-MM-DD\"T\"HH24:MI:SS.US\"+08:00\"') AS started_at,
                    to_char(turns.ended_at,
                        'YYYY-MM-DD\"T\"HH24:MI:SS.US\"+08:00\"') AS ended_at
             FROM turns turns
             LEFT JOIN trace_spans spans ON spans.trace_id = turns.id
             WHERE turns.id = $1
             GROUP BY turns.id",
        )
        .bind(turn_id.as_str())
        .fetch_optional(&self.pool)
        .await?
        .ok_or_else(|| StorageError::TurnNotFound(turn_id.to_string()))?;
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
             WHERE trace_id = $1
             ORDER BY started_at, id",
        )
        .bind(turn_id.as_str())
        .fetch_all(&self.pool)
        .await?;
        summary.span_count = i64::try_from(spans.len()).unwrap_or(i64::MAX);
        // 与 span_count 同理，以实际加载的 span 为准重算 token 合计。
        summary.total_tokens = spans.iter().filter_map(|span| span.total_tokens).sum();
        let completeness = derive_trace_completeness(
            &summary.status,
            summary.model_submission_count,
            summary.tool_call_count,
            &spans,
        );
        Ok(TurnTrace {
            summary,
            spans,
            completeness,
        })
    }

    pub async fn get_trace_by_id(&self, trace_id: &str) -> Result<TurnTrace, StorageError> {
        if trace_id.trim().is_empty() {
            return Err(StorageError::InvalidInput(
                "trace id must not be blank".to_string(),
            ));
        }
        let turn_id: Option<String> = sqlx::query_scalar(
            "SELECT id FROM turns WHERE id = $1
             UNION ALL
             SELECT turn_id FROM trace_spans
             WHERE trace_id = $1 AND turn_id IS NOT NULL
             LIMIT 1",
        )
        .bind(trace_id)
        .fetch_optional(&self.pool)
        .await?;
        if let Some(turn_id) = turn_id {
            return self.get_trace(&TurnId::new(turn_id)).await;
        }

        let mut summary = sqlx::query_as::<_, TraceTurnSummary>(
            "SELECT root.trace_id, NULL AS turn_id, root.session_id,
                    NULL AS turn_sequence,
                    CASE root.status
                        WHEN 'succeeded' THEN 'completed'
                        WHEN 'outcome_unknown' THEN 'interrupted'
                        ELSE root.status
                    END AS status,
                    COALESCE(root.resolved_model_name, '') AS resolved_model_name,
                    0 AS model_call_count, 0 AS model_submission_count,
                    0 AS tool_call_count,
                    (SELECT COUNT(*)::BIGINT FROM trace_spans member
                     WHERE member.trace_id = root.trace_id) AS span_count,
                    (SELECT COALESCE(SUM(member.input_tokens + member.output_tokens), 0)::BIGINT
                     FROM trace_spans member
                     WHERE member.trace_id = root.trace_id) AS total_tokens,
                    to_char(root.started_at,
                        'YYYY-MM-DD\"T\"HH24:MI:SS.US\"+08:00\"') AS started_at,
                    to_char(root.ended_at,
                        'YYYY-MM-DD\"T\"HH24:MI:SS.US\"+08:00\"') AS ended_at
             FROM trace_spans root
             WHERE root.trace_id = $1
               AND root.turn_id IS NULL
               AND root.parent_span_id IS NULL
             ORDER BY root.started_at, root.id
             LIMIT 1",
        )
        .bind(trace_id)
        .fetch_optional(&self.pool)
        .await?
        .ok_or_else(|| StorageError::TraceNotFound(trace_id.to_string()))?;
        let spans = self.load_trace_spans(trace_id).await?;
        summary.span_count = i64::try_from(spans.len()).unwrap_or(i64::MAX);
        // 与 span_count 同理，以实际加载的 span 为准重算 token 合计。
        summary.total_tokens = spans.iter().filter_map(|span| span.total_tokens).sum();
        let completeness = derive_trace_completeness(
            &summary.status,
            summary.model_submission_count,
            summary.tool_call_count,
            &spans,
        );
        Ok(TurnTrace {
            summary,
            spans,
            completeness,
        })
    }

    pub async fn get_span_payload(
        &self,
        span_id: &str,
        slot: TracePayloadSlot,
    ) -> Result<Option<TraceSpanPayloadRecord>, StorageError> {
        if span_id.trim().is_empty() {
            return Err(StorageError::InvalidInput(
                "span id must not be blank".to_string(),
            ));
        }
        Ok(sqlx::query_as::<_, TraceSpanPayloadRecord>(
            "SELECT mapping.span_id, mapping.slot, payload.body, payload.byte_size,
                    mapping.truncated, mapping.original_byte_size,
                    mapping.redacted_count
             FROM trace_span_payloads mapping
             JOIN trace_payloads payload ON payload.hash = mapping.payload_hash
             WHERE mapping.span_id = $1 AND mapping.slot = $2",
        )
        .bind(span_id)
        .bind(slot.as_str())
        .fetch_optional(&self.pool)
        .await?)
    }

    async fn load_trace_spans(&self, trace_id: &str) -> Result<Vec<TraceSpanRecord>, StorageError> {
        Ok(sqlx::query_as::<_, TraceSpanRecord>(
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
             WHERE trace_id = $1
             ORDER BY started_at, id",
        )
        .bind(trace_id)
        .fetch_all(&self.pool)
        .await?)
    }
}

fn derive_trace_completeness(
    turn_status: &str,
    expected_model_calls: i32,
    expected_tool_calls: i32,
    spans: &[TraceSpanRecord],
) -> TraceCompleteness {
    use std::collections::HashSet;

    let expected_model_calls = u32::try_from(expected_model_calls.max(0)).unwrap_or(u32::MAX);
    let expected_tool_calls = u32::try_from(expected_tool_calls.max(0)).unwrap_or(u32::MAX);
    let model_ids = spans
        .iter()
        .filter(|span| span.kind == "model_call")
        .map(|span| span.id.as_str())
        .collect::<HashSet<_>>();
    let captured_model_calls = saturating_u32(
        spans
            .iter()
            .filter(|span| span.kind == "model_call" && span.parent_span_id.is_none())
            .count(),
    );
    let captured_tool_calls =
        saturating_u32(spans.iter().filter(|span| span.kind == "tool_call").count());
    let orphan_tool_spans = saturating_u32(
        spans
            .iter()
            .filter(|span| {
                span.kind == "tool_call"
                    && span
                        .parent_span_id
                        .as_deref()
                        .is_none_or(|parent| !model_ids.contains(parent))
            })
            .count(),
    );
    let running_spans =
        saturating_u32(spans.iter().filter(|span| span.status == "running").count());
    let outcome_unknown_spans = saturating_u32(
        spans
            .iter()
            .filter(|span| span.status == "outcome_unknown")
            .count(),
    );
    let expected_total = expected_model_calls.saturating_add(expected_tool_calls);
    let captured_total = captured_model_calls.saturating_add(captured_tool_calls);
    let state = if expected_total > 0 && captured_total == 0 {
        TraceCompletenessState::None
    } else if turn_status != "running"
        && captured_model_calls == expected_model_calls
        && captured_tool_calls == expected_tool_calls
        && orphan_tool_spans == 0
        && running_spans == 0
        && outcome_unknown_spans == 0
    {
        TraceCompletenessState::Complete
    } else {
        TraceCompletenessState::Partial
    };
    TraceCompleteness {
        expected_model_calls,
        captured_model_calls,
        expected_tool_calls,
        captured_tool_calls,
        orphan_tool_spans,
        running_spans,
        outcome_unknown_spans,
        state,
    }
}

#[cfg(test)]
mod trace_completeness_tests {
    use serde_json::json;

    use super::*;

    fn span(id: &str, kind: &str, parent_span_id: Option<&str>, status: &str) -> TraceSpanRecord {
        TraceSpanRecord {
            id: id.to_string(),
            trace_id: "turn-1".to_string(),
            session_id: "session-1".to_string(),
            turn_id: Some("turn-1".to_string()),
            parent_span_id: parent_span_id.map(str::to_string),
            kind: kind.to_string(),
            name: format!("{kind}.call"),
            status: status.to_string(),
            model_id: None,
            resolved_model_name: None,
            provider_request_id: None,
            provider_call_id: None,
            requested_tool_name: None,
            resolved_tool_name: None,
            attempt_count: None,
            input_tokens: None,
            output_tokens: None,
            cached_input_tokens: None,
            reasoning_tokens: None,
            total_tokens: None,
            response_message_id: None,
            permission_wait_ms: None,
            started_at: "2026-07-19T00:00:00.000000Z".to_string(),
            ended_at: Some("2026-07-19T00:00:01.000000Z".to_string()),
            error_code: None,
            error_message: None,
            attributes: json!({}),
        }
    }

    #[test]
    fn derives_complete_partial_and_none_without_persisting_another_status() {
        let complete_spans = vec![
            span("model-1", "model_call", None, "succeeded"),
            span(
                "summary-model-1",
                "model_call",
                Some("compaction-1"),
                "succeeded",
            ),
            span("tool-1", "tool_call", Some("model-1"), "succeeded"),
        ];
        let complete = derive_trace_completeness("completed", 1, 1, &complete_spans);
        assert_eq!(complete.state, TraceCompletenessState::Complete);
        assert_eq!(complete.captured_model_calls, 1);
        assert_eq!(complete.captured_tool_calls, 1);

        let none = derive_trace_completeness("failed", 1, 1, &[]);
        assert_eq!(none.state, TraceCompletenessState::None);

        let orphan = vec![
            span("model-1", "model_call", None, "succeeded"),
            span("tool-1", "tool_call", Some("missing"), "succeeded"),
        ];
        let partial = derive_trace_completeness("completed", 1, 1, &orphan);
        assert_eq!(partial.state, TraceCompletenessState::Partial);
        assert_eq!(partial.orphan_tool_spans, 1);

        let running = derive_trace_completeness("running", 1, 1, &complete_spans);
        assert_eq!(running.state, TraceCompletenessState::Partial);
    }
}
