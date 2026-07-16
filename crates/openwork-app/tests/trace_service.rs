use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use async_trait::async_trait;
use openwork_app::{
    TraceApplicationService, TraceDataCompleteness, TraceDiagnosisReason, TraceDiagnosisStatus,
    TraceListQuery, TraceSpanDetail,
};
use openwork_persistence::SessionStore;
use openwork_protocol::trace::{
    TraceRepository, TraceRepositoryError, TraceSpan, TraceSpanKind, TraceSpanStatus,
};
use openwork_protocol::{
    domain::EventId,
    journal::{
        AggregateType, EventJournal, EventJournalError, ExpectedVersion, NewRecordedEventV1,
        RecordedEventV1,
    },
};
use serde_json::json;

#[derive(Default)]
struct MemoryTraceRepository {
    spans: Mutex<HashMap<String, TraceSpan>>,
}

#[async_trait]
impl TraceRepository for MemoryTraceRepository {
    async fn upsert_span(&self, span: TraceSpan) -> Result<(), TraceRepositoryError> {
        self.spans
            .lock()
            .unwrap()
            .insert(span.span_id.clone(), span);
        Ok(())
    }
    async fn load_span(&self, span_id: &str) -> Result<Option<TraceSpan>, TraceRepositoryError> {
        Ok(self.spans.lock().unwrap().get(span_id).cloned())
    }
    async fn load_turn(&self, turn_id: &str) -> Result<Vec<TraceSpan>, TraceRepositoryError> {
        Ok(self
            .spans
            .lock()
            .unwrap()
            .values()
            .filter(|span| span.turn_id == turn_id)
            .cloned()
            .collect())
    }
    async fn load_session(&self, session_id: &str) -> Result<Vec<TraceSpan>, TraceRepositoryError> {
        Ok(self
            .spans
            .lock()
            .unwrap()
            .values()
            .filter(|span| span.session_id == session_id)
            .cloned()
            .collect())
    }
    async fn load_recent_turns(
        &self,
        limit: u32,
        offset: u32,
    ) -> Result<Vec<TraceSpan>, TraceRepositoryError> {
        let spans = self.spans.lock().unwrap();
        let mut roots = spans
            .values()
            .filter(|span| span.span_kind == TraceSpanKind::Turn)
            .cloned()
            .collect::<Vec<_>>();
        roots.sort_by(|left, right| right.started_at_unix_ms.cmp(&left.started_at_unix_ms));
        let turn_ids = roots
            .into_iter()
            .skip(offset as usize)
            .take(limit as usize)
            .map(|span| span.turn_id)
            .collect::<Vec<_>>();
        Ok(spans
            .values()
            .filter(|span| turn_ids.contains(&span.turn_id))
            .cloned()
            .collect())
    }
}

fn span(
    id: &str,
    kind: TraceSpanKind,
    parent: Option<&str>,
    attributes: serde_json::Value,
) -> TraceSpan {
    TraceSpan {
        trace_id: "turn-1".to_string(),
        span_id: id.to_string(),
        parent_span_id: parent.map(str::to_string),
        span_kind: kind,
        span_name: kind.as_str().to_string(),
        status: TraceSpanStatus::Succeeded,
        session_id: "session-1".to_string(),
        turn_id: "turn-1".to_string(),
        step_id: None,
        tool_run_id: None,
        started_at_unix_ms: 1_000,
        ended_at_unix_ms: Some(2_000),
        attributes,
        error_type: None,
        error_code: None,
        error_message: None,
    }
}

#[tokio::test]
async fn service_builds_one_lightweight_turn_summary() {
    let repository = Arc::new(MemoryTraceRepository::default());
    for item in [
        span(
            "turn-1",
            TraceSpanKind::Turn,
            None,
            json!({"model": "glm-5.1"}),
        ),
        span("step-1", TraceSpanKind::Step, Some("turn-1"), json!({})),
        span(
            "model-1",
            TraceSpanKind::ModelAttempt,
            Some("step-1"),
            json!({"inputTokens": 120, "outputTokens": 30}),
        ),
        span(
            "transport-1",
            TraceSpanKind::TransportAttempt,
            Some("model-1"),
            json!({}),
        ),
        span(
            "transport-2",
            TraceSpanKind::TransportAttempt,
            Some("model-1"),
            json!({}),
        ),
        span("tool-1", TraceSpanKind::ToolRun, Some("step-1"), json!({})),
    ] {
        repository.upsert_span(item).await.unwrap();
    }
    let service = TraceApplicationService::new(repository);

    let summaries = service.list_session("session-1").await.unwrap();

    assert_eq!(summaries.len(), 1);
    assert_eq!(summaries[0].step_count, 1);
    assert_eq!(summaries[0].model_attempt_count, 1);
    assert_eq!(summaries[0].transport_attempt_count, 2);
    assert_eq!(summaries[0].retry_count, 1);
    assert_eq!(summaries[0].input_tokens, 120);
    assert_eq!(summaries[0].output_tokens, 30);
}

#[tokio::test]
async fn service_pages_recent_turns_without_splitting_one_trace() {
    let repository = Arc::new(MemoryTraceRepository::default());
    for (turn_id, started_at) in [("turn-old", 1_000), ("turn-new", 2_000)] {
        repository
            .upsert_span(TraceSpan {
                trace_id: turn_id.to_string(),
                span_id: turn_id.to_string(),
                parent_span_id: None,
                span_kind: TraceSpanKind::Turn,
                span_name: "turn.run".to_string(),
                status: TraceSpanStatus::Succeeded,
                session_id: "session-1".to_string(),
                turn_id: turn_id.to_string(),
                step_id: None,
                tool_run_id: None,
                started_at_unix_ms: started_at,
                ended_at_unix_ms: Some(started_at + 500),
                attributes: json!({"model": "glm-5.1"}),
                error_type: None,
                error_code: None,
                error_message: None,
            })
            .await
            .unwrap();
    }
    let service = TraceApplicationService::new(repository);

    let first = service.list_recent(1, 0).await.unwrap();
    assert_eq!(first.items[0].turn_id, "turn-new");
    assert_eq!(first.next_offset, Some(1));

    let second = service
        .list_recent(1, first.next_offset.unwrap())
        .await
        .unwrap();
    assert_eq!(second.items[0].turn_id, "turn-old");
    assert_eq!(second.next_offset, None);
}

#[tokio::test]
async fn diagnosis_uses_terminal_root_state_and_preserves_retry_evidence() {
    let repository = Arc::new(MemoryTraceRepository::default());
    let root = span(
        "turn-1",
        TraceSpanKind::Turn,
        None,
        json!({"model": "glm-5.1", "traceSchemaVersion": "1.1"}),
    );
    repository.upsert_span(root).await.unwrap();
    let mut failed_transport = span(
        "transport-1",
        TraceSpanKind::TransportAttempt,
        Some("model-1"),
        json!({"transportAttempt": 1, "willRetry": true}),
    );
    failed_transport.status = TraceSpanStatus::Failed;
    repository.upsert_span(failed_transport).await.unwrap();
    repository
        .upsert_span(span(
            "model-1",
            TraceSpanKind::ModelAttempt,
            Some("turn-1"),
            json!({}),
        ))
        .await
        .unwrap();
    repository
        .upsert_span(span(
            "transport-2",
            TraceSpanKind::TransportAttempt,
            Some("model-1"),
            json!({"transportAttempt": 2}),
        ))
        .await
        .unwrap();

    let trace = TraceApplicationService::new(repository)
        .load_turn("turn-1")
        .await
        .unwrap();

    assert_eq!(
        trace.summary.diagnosis.status,
        TraceDiagnosisStatus::Healthy
    );
    assert_eq!(
        trace.summary.diagnosis.reason,
        TraceDiagnosisReason::Healthy
    );
    assert_eq!(trace.summary.retry_count, 1);
    assert_eq!(
        trace.summary.data_completeness,
        TraceDataCompleteness::Complete
    );
}

#[tokio::test]
async fn diagnosis_marks_a_completed_turn_with_denied_approval_as_recovered() {
    let repository = Arc::new(MemoryTraceRepository::default());
    repository
        .upsert_span(span(
            "turn-1",
            TraceSpanKind::Turn,
            None,
            json!({"traceSchemaVersion": "1.1"}),
        ))
        .await
        .unwrap();
    let mut approval = span(
        "approval-1",
        TraceSpanKind::Approval,
        Some("turn-1"),
        json!({"resolution": "deny"}),
    );
    approval.status = TraceSpanStatus::Denied;
    repository.upsert_span(approval).await.unwrap();
    repository
        .upsert_span(span(
            "recovery-1",
            TraceSpanKind::Recovery,
            Some("turn-1"),
            json!({"approvalId": "approval-1"}),
        ))
        .await
        .unwrap();

    let trace = TraceApplicationService::new(repository)
        .load_turn("turn-1")
        .await
        .unwrap();

    assert_eq!(
        trace.summary.diagnosis.status,
        TraceDiagnosisStatus::Attention
    );
    assert_eq!(
        trace.summary.diagnosis.reason,
        TraceDiagnosisReason::Recovered
    );
    assert_eq!(
        trace.summary.diagnosis.focus_span_id.as_deref(),
        Some("recovery-1")
    );
}

#[tokio::test]
async fn diagnosis_marks_orphaned_span_trees_as_partial() {
    let repository = Arc::new(MemoryTraceRepository::default());
    repository
        .upsert_span(span(
            "tool-1",
            TraceSpanKind::ToolRun,
            Some("missing-step"),
            json!({"toolName": "bash"}),
        ))
        .await
        .unwrap();

    let trace = TraceApplicationService::new(repository)
        .load_turn("turn-1")
        .await
        .unwrap();

    assert_eq!(
        trace.summary.data_completeness,
        TraceDataCompleteness::Partial
    );
    assert_eq!(
        trace.summary.diagnosis.reason,
        TraceDiagnosisReason::PartialTrace
    );
    assert!(
        trace
            .summary
            .diagnosis
            .evidence_span_ids
            .contains(&"tool-1".to_string())
    );
}

#[derive(Default)]
struct ReadOnlyEventJournal {
    events: Vec<RecordedEventV1>,
}

#[async_trait]
impl EventJournal for ReadOnlyEventJournal {
    async fn append(
        &self,
        _aggregate_type: AggregateType,
        _aggregate_id: &str,
        _expected_version: ExpectedVersion,
        _events: Vec<NewRecordedEventV1>,
    ) -> Result<Vec<RecordedEventV1>, EventJournalError> {
        Err(EventJournalError::Persistence {
            message: "read only".to_string(),
        })
    }

    async fn load_aggregate(
        &self,
        aggregate_type: AggregateType,
        aggregate_id: &str,
        after_version: u64,
    ) -> Result<Vec<RecordedEventV1>, EventJournalError> {
        Ok(self
            .events
            .iter()
            .filter(|event| {
                event.aggregate_type == aggregate_type
                    && event.aggregate_id == aggregate_id
                    && event.aggregate_version > after_version
            })
            .cloned()
            .collect())
    }

    async fn read_all(
        &self,
        after_global_position: u64,
        limit: u32,
    ) -> Result<Vec<RecordedEventV1>, EventJournalError> {
        Ok(self
            .events
            .iter()
            .filter(|event| event.global_position > after_global_position)
            .take(limit as usize)
            .cloned()
            .collect())
    }
}

fn journal_event(
    position: u64,
    aggregate_type: AggregateType,
    aggregate_id: &str,
    event_type: &str,
    payload: serde_json::Value,
) -> RecordedEventV1 {
    RecordedEventV1 {
        global_position: position,
        event_id: EventId::new(format!("event-{position}")),
        aggregate_type,
        aggregate_id: aggregate_id.to_string(),
        aggregate_version: position,
        event_type: event_type.to_string(),
        event_version: 1,
        payload,
        occurred_at_unix_ms: position as i64 * 1_000,
        recorded_at_unix_ms: position as i64 * 1_000,
    }
}

fn trace_context_store() -> SessionStore {
    SessionStore::new(Arc::new(ReadOnlyEventJournal {
        events: vec![
            journal_event(
                1,
                AggregateType::Session,
                "session-1",
                "session_created",
                json!({
                    "title": "Fix trace UI",
                    "providerId": "provider-1",
                    "model": "glm-5.1",
                    "workingDir": "/Volumes/Code/OpenWork"
                }),
            ),
            journal_event(
                2,
                AggregateType::Turn,
                "turn-1",
                "turn_started",
                json!({
                    "sessionId": "session-1",
                    "providerId": "provider-1",
                    "model": "glm-5.1"
                }),
            ),
            journal_event(
                3,
                AggregateType::Turn,
                "turn-1",
                "user_message_recorded",
                json!({
                    "messageId": "message-user",
                    "sessionId": "session-1",
                    "role": "user",
                    "parts": [{"type": "text", "text": "run cargo test"}]
                }),
            ),
            journal_event(
                4,
                AggregateType::Turn,
                "turn-1",
                "step_started",
                json!({"stepId": "step-1", "stepIndex": 1}),
            ),
            journal_event(
                5,
                AggregateType::Turn,
                "turn-1",
                "tool_run_requested",
                json!({
                    "stepId": "step-1",
                    "toolRunId": "tool-run-1",
                    "providerToolCallId": "call-1",
                    "toolName": "bash",
                    "input": {"command": "cargo test"}
                }),
            ),
            journal_event(
                6,
                AggregateType::Turn,
                "turn-1",
                "tool_run_started",
                json!({"stepId": "step-1", "toolRunId": "tool-run-1"}),
            ),
            journal_event(
                7,
                AggregateType::Turn,
                "turn-1",
                "tool_run_completed",
                json!({
                    "stepId": "step-1",
                    "toolRunId": "tool-run-1",
                    "observation": {
                        "status": "succeeded",
                        "content": [{"type": "text", "text": "tests passed"}],
                        "error": null
                    }
                }),
            ),
        ],
    }))
}

#[tokio::test]
async fn typed_tool_detail_joins_input_and_observation_from_the_journal() {
    let repository = Arc::new(MemoryTraceRepository::default());
    let mut root = span(
        "turn-1",
        TraceSpanKind::Turn,
        None,
        json!({"model": "glm-5.1"}),
    );
    root.started_at_unix_ms = 1_000;
    root.ended_at_unix_ms = Some(14_000);
    repository.upsert_span(root).await.unwrap();
    let mut step = span(
        "step-1",
        TraceSpanKind::Step,
        Some("turn-1"),
        json!({"stepIndex": 1}),
    );
    step.step_id = Some("step-1".to_string());
    repository.upsert_span(step).await.unwrap();
    let mut tool = span(
        "tool-1",
        TraceSpanKind::ToolRun,
        Some("step-1"),
        json!({
            "toolName": "bash",
            "providerToolCallId": "call-1",
            "approvalRequired": true,
            "executionStartedAt": 13_980
        }),
    );
    tool.step_id = Some("step-1".to_string());
    tool.tool_run_id = Some("tool-run-1".to_string());
    tool.started_at_unix_ms = 1_000;
    tool.ended_at_unix_ms = Some(14_000);
    repository.upsert_span(tool).await.unwrap();
    let mut approval = span(
        "approval-1",
        TraceSpanKind::Approval,
        Some("tool-1"),
        json!({"reason": "process execution", "resolution": "allow"}),
    );
    approval.started_at_unix_ms = 1_010;
    approval.ended_at_unix_ms = Some(13_975);
    repository.upsert_span(approval).await.unwrap();

    let service = TraceApplicationService::with_session_store(repository, trace_context_store());
    let detail = service.load_span_detail("turn-1", "tool-1").await.unwrap();

    let TraceSpanDetail::ToolRun(tool) = detail.detail else {
        panic!("expected tool detail");
    };
    assert_eq!(tool.input, Some(json!({"command": "cargo test"})));
    assert_eq!(tool.observation.unwrap().text_content(), "tests passed");
    assert_eq!(tool.request_to_end_ms, Some(13_000));
    assert_eq!(tool.approval_wait_ms, Some(12_965));
    assert_eq!(tool.execution_ms, Some(20));
}

#[tokio::test]
async fn typed_turn_and_model_details_expose_outcome_and_content_free_request_shape() {
    let repository = Arc::new(MemoryTraceRepository::default());
    repository
        .upsert_span(span(
            "turn-1",
            TraceSpanKind::Turn,
            None,
            json!({
                "model": "glm-5.1",
                "outcome": "completed",
                "traceSchemaVersion": "1.1"
            }),
        ))
        .await
        .unwrap();
    repository
        .upsert_span(span(
            "model-1",
            TraceSpanKind::ModelAttempt,
            Some("turn-1"),
            json!({
                "providerId": "provider-1",
                "model": "glm-5.1",
                "requestSummaryVersion": "1",
                "messageCount": 4,
                "messageTextChars": 320,
                "systemPromptChars": 120,
                "toolDefinitionCount": 2,
                "toolNames": ["read", "bash"],
                "temperature": 0.2,
                "maxOutputTokens": 2048,
                "thinkingMode": "enabled"
            }),
        ))
        .await
        .unwrap();
    let service = TraceApplicationService::new(repository);

    let turn = service.load_span_detail("turn-1", "turn-1").await.unwrap();
    let TraceSpanDetail::Turn(turn) = turn.detail else {
        panic!("expected turn detail");
    };
    assert_eq!(turn.outcome.as_deref(), Some("completed"));

    let model = service.load_span_detail("turn-1", "model-1").await.unwrap();
    let TraceSpanDetail::ModelAttempt(model) = model.detail else {
        panic!("expected model detail");
    };
    assert_eq!(model.request_summary.message_count, Some(4));
    assert_eq!(model.request_summary.message_text_chars, Some(320));
    assert_eq!(model.request_summary.tool_names, vec!["read", "bash"]);
    assert_eq!(model.request_summary.max_output_tokens, Some(2_048));
    assert_eq!(
        model.request_summary.thinking_mode.as_deref(),
        Some("enabled")
    );
}

#[tokio::test]
async fn list_query_filters_before_paging_and_adds_session_context() {
    let repository = Arc::new(MemoryTraceRepository::default());
    repository
        .upsert_span(span(
            "turn-1",
            TraceSpanKind::Turn,
            None,
            json!({"model": "glm-5.1"}),
        ))
        .await
        .unwrap();
    let service = TraceApplicationService::with_session_store(repository, trace_context_store());

    let page = service
        .query_recent(TraceListQuery {
            limit: 10,
            query: Some("cargo test".to_string()),
            status: Some(TraceSpanStatus::Succeeded),
            ..TraceListQuery::default()
        })
        .await
        .unwrap();

    assert_eq!(page.items.len(), 1);
    assert_eq!(
        page.items[0].input_preview.as_deref(),
        Some("run cargo test")
    );
    assert_eq!(page.items[0].session_title.as_deref(), Some("Fix trace UI"));
    assert_eq!(
        page.items[0].working_dir.as_deref(),
        Some("/Volumes/Code/OpenWork")
    );
}
