use openwork_core::{
    ClientRequestId, CompactionFinished, CompactionStarted, CompactionTraceAttributesV1,
    ModelCallFinished, ModelCallStarted, ModelCallTraceGuard, ModelTraceAttributesV1,
    PostgresStorage, PostgresTraceRecorder, ResolvedModel, SessionId, SessionInput, SessionStorage,
    ToolCallStarted, ToolCallTraceGuard, ToolTraceAttributesV1, TraceContentConfig,
    TracePayloadSlot, TracePayloads, TraceRecorder, TraceSignal, TraceStatus, TurnOutcome,
    session::TurnId,
};
use openwork_models::model::{
    FinishReason, Message, ModelRequest, ModelResponse, Role, TokenUsage, ToolDefinition,
};
use openwork_tools::ToolResult;
use serde_json::{Value, json};
use time::OffsetDateTime;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

fn test_database_url() -> Option<String> {
    std::env::var("TEST_DATABASE_URL").ok()
}

fn unique(prefix: &str) -> String {
    format!("{prefix}-{}", Uuid::new_v4().simple())
}

async fn storage() -> Option<PostgresStorage> {
    let database_url = test_database_url()?;
    let storage = PostgresStorage::connect(Some(&database_url)).await.unwrap();
    storage.migrate().await.unwrap();
    Some(storage)
}

async fn create_session(storage: &PostgresStorage, prefix: &str) -> SessionId {
    let session_id = SessionId::new(unique(prefix));
    storage
        .create_session(&SessionInput {
            id: session_id.clone(),
            title: Some(prefix.to_string()),
            working_directory: format!("/tmp/{prefix}"),
            default_model_id: None,
        })
        .await
        .unwrap();
    session_id
}

async fn create_turn(storage: &PostgresStorage, prefix: &str) -> (SessionId, TurnId) {
    let session_id = create_session(storage, prefix).await;
    let turn_id = TurnId::new(unique("turn-payload"));
    storage
        .begin_turn(
            &session_id,
            &turn_id,
            &ClientRequestId::new(unique("request-payload")),
            &ResolvedModel::new(None::<String>, "test", "test-model"),
            &[],
            &Message::text(Role::User, "inspect trace payloads"),
        )
        .await
        .unwrap();
    (session_id, turn_id)
}

fn request(marker: &str) -> ModelRequest {
    ModelRequest {
        model: "test-model".to_string(),
        messages: vec![
            Message::text(Role::System, "stable system"),
            Message::text(Role::User, marker),
        ],
        temperature: None,
        top_p: None,
        max_output_tokens: None,
        thinking: None,
        tools: vec![ToolDefinition {
            name: "read".to_string(),
            description: "Read one file".to_string(),
            parameters: json!({"type": "object", "properties": {"path": {"type": "string"}}}),
        }],
    }
}

fn payloads(request: &ModelRequest) -> TracePayloads {
    let mut payloads = TracePayloads::for_model_request(request);
    payloads.system_context = Some(json!([
        {"key": "core/agent-system", "content": [{"type": "text", "text": "stable system"}]}
    ]));
    payloads
}

fn model_started(
    session_id: &SessionId,
    turn_id: Option<&TurnId>,
    trace_id: &str,
    parent_span_id: Option<String>,
    span_id: String,
    request: &ModelRequest,
) -> ModelCallStarted {
    ModelCallStarted {
        span_id,
        trace_id: trace_id.to_string(),
        session_id: session_id.clone(),
        turn_id: turn_id.cloned(),
        parent_span_id,
        model_id: None,
        resolved_model_name: request.model.clone(),
        started_at: OffsetDateTime::now_utc(),
        attributes: ModelTraceAttributesV1::from_request(1, 1, request),
        payloads: payloads(request),
    }
}

fn finished_model(started: ModelCallStarted, response: Option<Value>) -> TraceSignal {
    TraceSignal::ModelCallFinished(Box::new(ModelCallFinished {
        attributes: started.attributes.clone(),
        started,
        status: TraceStatus::Succeeded,
        provider_request_id: Some("provider-request".to_string()),
        attempt_count: Some(1),
        usage: Some(TokenUsage {
            input_tokens: Some(11),
            output_tokens: Some(7),
            total_tokens: Some(18),
            cached_input_tokens: None,
            cache_creation_input_tokens: None,
            reasoning_tokens: None,
        }),
        ended_at: OffsetDateTime::now_utc(),
        error_code: None,
        error_message: None,
        response_message_id: None,
        response_payload: response,
    }))
}

fn compaction(session_id: &SessionId, trace_id: &str, span_id: String) -> CompactionStarted {
    CompactionStarted {
        span_id,
        trace_id: trace_id.to_string(),
        session_id: session_id.clone(),
        turn_id: None,
        model_id: None,
        resolved_model_name: "test-model".to_string(),
        started_at: OffsetDateTime::now_utc(),
        attributes: CompactionTraceAttributesV1::new("manual"),
    }
}

fn finish_compaction(started: CompactionStarted) -> TraceSignal {
    TraceSignal::CompactionFinished(Box::new(CompactionFinished {
        attributes: started.attributes.clone(),
        started,
        status: TraceStatus::Succeeded,
        attempt_count: Some(1),
        input_tokens: Some(11),
        output_tokens: Some(7),
        ended_at: OffsetDateTime::now_utc(),
        error_code: None,
        error_message: None,
    }))
}

fn response(text: &str) -> ModelResponse {
    ModelResponse {
        response_id: Some(unique("response")),
        provider_request_id: Some(unique("provider-request")),
        model: Some("test-model".to_string()),
        text: text.to_string(),
        reasoning_text: None,
        tool_calls: Vec::new(),
        provider_opaque_blocks: Vec::new(),
        finish_reason: FinishReason::Stop,
        raw_finish_reason: Some("stop".to_string()),
        usage: Some(TokenUsage {
            input_tokens: Some(4),
            output_tokens: Some(2),
            total_tokens: Some(6),
            cached_input_tokens: None,
            cache_creation_input_tokens: None,
            reasoning_tokens: None,
        }),
    }
}

#[tokio::test]
async fn payloads_are_deduplicated_loaded_on_demand_and_kept_out_of_trace_reads() {
    let Some(storage) = storage().await else {
        return;
    };
    let session_id = create_session(&storage, "session-payload-dedup").await;
    let trace_id = unique("trace-payload-dedup");
    let root = compaction(&session_id, &trace_id, unique("span-compaction"));
    let root_id = root.span_id.clone();
    let first_id = unique("span-summary");
    let second_id = unique("span-summary");
    let request = request("request-visible-only-on-demand");
    let recorder = PostgresTraceRecorder::spawn(storage.pool().clone());
    recorder.record(finish_compaction(root));
    for span_id in [&first_id, &second_id] {
        recorder.record(finished_model(
            model_started(
                &session_id,
                None,
                &trace_id,
                Some(root_id.clone()),
                span_id.clone(),
                &request,
            ),
            Some(json!({"text": "summary response"})),
        ));
    }
    let flush = recorder.flush_session(&session_id).await;
    assert!(flush.flushed);
    assert_eq!(flush.write_failures, 0);

    let loaded = storage
        .get_span_payload(&first_id, TracePayloadSlot::Request)
        .await
        .unwrap()
        .expect("request payload");
    assert_eq!(
        loaded.body,
        serde_json::to_value(&request.messages).unwrap()
    );
    assert!(!loaded.truncated);
    assert_eq!(loaded.original_byte_size, None);
    assert_eq!(loaded.redacted_count, 0);
    let system_context = storage
        .get_span_payload(&first_id, TracePayloadSlot::SystemContext)
        .await
        .unwrap()
        .expect("system context payload");
    assert_eq!(system_context.body[0]["key"], "core/agent-system");
    let tool_definitions = storage
        .get_span_payload(&first_id, TracePayloadSlot::ToolDefinitions)
        .await
        .unwrap()
        .expect("tool definitions payload");
    assert_eq!(
        tool_definitions.body,
        serde_json::to_value(&request.tools).unwrap()
    );
    assert!(
        storage
            .get_span_payload(&first_id, TracePayloadSlot::Response)
            .await
            .unwrap()
            .is_some()
    );

    let (mappings, distinct_hashes): (i64, i64) = sqlx::query_as(
        "SELECT COUNT(*)::BIGINT, COUNT(DISTINCT payload_hash)::BIGINT
         FROM trace_span_payloads
         WHERE slot = 'tool_definitions' AND span_id IN ($1, $2)",
    )
    .bind(&first_id)
    .bind(&second_id)
    .fetch_one(storage.pool())
    .await
    .unwrap();
    assert_eq!((mappings, distinct_hashes), (2, 1));

    let trace = storage.get_trace_by_id(&trace_id).await.unwrap();
    assert_eq!(trace.summary.trace_id, trace_id);
    assert_eq!(trace.summary.turn_id, None);
    assert_eq!(trace.spans.len(), 3);
    let encoded = serde_json::to_string(&trace).unwrap();
    assert!(!encoded.contains("request-visible-only-on-demand"));
    assert!(!encoded.contains("summary response"));

    storage.delete_session(&session_id).await.unwrap();
}

#[tokio::test]
async fn oversized_payloads_are_truncated_and_the_database_enforces_truncation_metadata() {
    let Some(storage) = storage().await else {
        return;
    };
    let session_id = create_session(&storage, "session-payload-truncation").await;
    let trace_id = unique("trace-payload-truncation");
    let root = compaction(&session_id, &trace_id, unique("span-compaction"));
    let root_id = root.span_id.clone();
    let model_id = unique("span-summary");
    let request = request(&"x".repeat(4_096));
    let recorder = PostgresTraceRecorder::spawn_with_content_config(
        storage.pool().clone(),
        TraceContentConfig::new(128, 30).unwrap(),
    );
    recorder.record(finish_compaction(root));
    recorder.record(finished_model(
        model_started(
            &session_id,
            None,
            &trace_id,
            Some(root_id.clone()),
            model_id.clone(),
            &request,
        ),
        None,
    ));
    let flush = recorder.flush_session(&session_id).await;
    assert!(flush.flushed);
    assert_eq!(flush.write_failures, 0);

    let loaded = storage
        .get_span_payload(&model_id, TracePayloadSlot::Request)
        .await
        .unwrap()
        .expect("truncated request");
    assert!(loaded.truncated);
    assert!(loaded.original_byte_size.is_some_and(|size| size > 128));
    assert!(loaded.byte_size <= 128);
    assert_eq!(loaded.redacted_count, 0);

    let payload_hash: String = sqlx::query_scalar(
        "SELECT payload_hash FROM trace_span_payloads
         WHERE span_id = $1 AND slot = 'request'",
    )
    .bind(&model_id)
    .fetch_one(storage.pool())
    .await
    .unwrap();
    let invalid = sqlx::query(
        "INSERT INTO trace_span_payloads (
             span_id, slot, payload_hash, truncated, original_byte_size, redacted_count
         ) VALUES ($1, 'request', $2, TRUE, NULL, 0)",
    )
    .bind(&root_id)
    .bind(payload_hash)
    .execute(storage.pool())
    .await
    .expect_err("truncated payload without original size must be rejected");
    assert_eq!(
        invalid
            .as_database_error()
            .and_then(|error| error.code())
            .as_deref(),
        Some("23514")
    );

    storage.delete_session(&session_id).await.unwrap();
}

#[tokio::test]
async fn model_and_tool_response_storage_follows_message_pointer_rules() {
    let Some(storage) = storage().await else {
        return;
    };
    let (session_id, turn_id) = create_turn(&storage, "session-response-pointer").await;
    let request = request("pointer-request-only-on-demand");
    let recorder = PostgresTraceRecorder::spawn(storage.pool().clone());
    let successful_started = model_started(
        &session_id,
        Some(&turn_id),
        turn_id.as_str(),
        None,
        unique("span-model-success"),
        &request,
    );
    let successful_id = successful_started.span_id.clone();
    let successful_response = response("durable assistant response");
    let guard = ModelCallTraceGuard::start(
        std::sync::Arc::new(recorder.clone()),
        successful_started,
        CancellationToken::new(),
        1,
    );
    let message_id = storage
        .append_assistant_message(
            &turn_id,
            &Message::text(Role::Assistant, &successful_response.text),
            successful_response.usage,
        )
        .await
        .unwrap();
    guard.finish_success_with_message(&successful_response, message_id.clone());

    let failed_started = model_started(
        &session_id,
        Some(&turn_id),
        turn_id.as_str(),
        None,
        unique("span-model-failed"),
        &request,
    );
    let failed_id = failed_started.span_id.clone();
    let mut failed_guard = ModelCallTraceGuard::start(
        std::sync::Arc::new(recorder.clone()),
        failed_started,
        CancellationToken::new(),
        1,
    );
    failed_guard.record_first_semantic_event();
    failed_guard.record_text_delta("partial response kept only by trace");
    failed_guard.finish_failure(
        TraceStatus::Failed,
        None,
        "stream_failed",
        "stream failed after semantic output",
        None,
    );

    let successful_tool_id = unique("span-tool-success");
    let successful_tool = ToolCallTraceGuard::start(
        std::sync::Arc::new(recorder.clone()),
        ToolCallStarted {
            span_id: successful_tool_id.clone(),
            trace_id: turn_id.to_string(),
            turn_id: turn_id.clone(),
            parent_span_id: successful_id.clone(),
            provider_call_id: unique("provider-call"),
            requested_tool_name: "read".to_string(),
            started_at: OffsetDateTime::now_utc(),
            attributes: ToolTraceAttributesV1::new(),
        },
        CancellationToken::new(),
    );
    successful_tool.finish_result(&ToolResult::succeeded("file contents"), true);

    let denied_tool_id = unique("span-tool-denied");
    let denied_tool = ToolCallTraceGuard::start(
        std::sync::Arc::new(recorder.clone()),
        ToolCallStarted {
            span_id: denied_tool_id.clone(),
            trace_id: turn_id.to_string(),
            turn_id: turn_id.clone(),
            parent_span_id: successful_id.clone(),
            provider_call_id: unique("provider-call"),
            requested_tool_name: "write".to_string(),
            started_at: OffsetDateTime::now_utc(),
            attributes: ToolTraceAttributesV1::new(),
        },
        CancellationToken::new(),
    );
    denied_tool.finish_result(&ToolResult::denied("user denied permission"), true);

    let flush = recorder.flush_turn(&turn_id).await;
    assert!(flush.flushed);
    assert_eq!(flush.write_failures, 0);
    let stored_pointer: Option<String> =
        sqlx::query_scalar("SELECT response_message_id FROM trace_spans WHERE id = $1")
            .bind(&successful_id)
            .fetch_one(storage.pool())
            .await
            .unwrap();
    assert_eq!(stored_pointer.as_deref(), Some(message_id.as_str()));
    assert!(
        storage
            .get_span_payload(&successful_id, TracePayloadSlot::Response)
            .await
            .unwrap()
            .is_none()
    );
    let failed_response = storage
        .get_span_payload(&failed_id, TracePayloadSlot::Response)
        .await
        .unwrap()
        .expect("failed partial response");
    assert_eq!(
        failed_response.body["text"],
        "partial response kept only by trace"
    );
    let successful_tool_payloads: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM trace_span_payloads WHERE span_id = $1")
            .bind(&successful_tool_id)
            .fetch_one(storage.pool())
            .await
            .unwrap();
    assert_eq!(successful_tool_payloads, 0);
    assert!(
        storage
            .get_span_payload(&denied_tool_id, TracePayloadSlot::Response)
            .await
            .unwrap()
            .is_some()
    );

    let trace = storage.get_trace(&turn_id).await.unwrap();
    assert!(trace.spans.iter().any(|span| span.id == successful_id
        && span.response_message_id.as_deref() == Some(message_id.as_str())));
    let encoded = serde_json::to_string(&trace).unwrap();
    assert!(!encoded.contains("pointer-request-only-on-demand"));
    assert!(!encoded.contains("partial response kept only by trace"));

    storage
        .finish_turn(
            &turn_id,
            &TurnOutcome::Completed {
                final_text: "done".to_string(),
            },
            None,
        )
        .await
        .unwrap();
    storage.delete_session(&session_id).await.unwrap();
}

#[tokio::test]
async fn payload_write_failure_keeps_the_span() {
    let Some(storage) = storage().await else {
        return;
    };
    let (session_id, turn_id) = create_turn(&storage, "session-payload-failure").await;
    let marker = Uuid::new_v4().simple().to_string();
    let function_name = format!("reject_trace_payload_{}", Uuid::new_v4().simple());
    let trigger_name = format!("reject_trace_payload_{}", Uuid::new_v4().simple());
    sqlx::query(&format!(
        "CREATE FUNCTION {function_name}() RETURNS trigger LANGUAGE plpgsql AS $$
         BEGIN
             IF NEW.body ->> 'forcePayloadFailure' = '{marker}' THEN
                 RAISE EXCEPTION 'forced trace payload failure';
             END IF;
             RETURN NEW;
         END
         $$"
    ))
    .execute(storage.pool())
    .await
    .unwrap();
    sqlx::query(&format!(
        "CREATE TRIGGER {trigger_name}
         BEFORE INSERT ON trace_payloads
         FOR EACH ROW EXECUTE FUNCTION {function_name}()"
    ))
    .execute(storage.pool())
    .await
    .unwrap();

    let request = request("payload failure request");
    let mut started = model_started(
        &session_id,
        Some(&turn_id),
        turn_id.as_str(),
        None,
        unique("span-payload-failure"),
        &request,
    );
    started.payloads = TracePayloads {
        request: Some(json!({"forcePayloadFailure": marker})),
        system_context: None,
        tool_definitions: None,
    };
    let span_id = started.span_id.clone();
    let recorder = PostgresTraceRecorder::spawn(storage.pool().clone());
    recorder.record(finished_model(started, None));
    let flush = recorder.flush_turn(&turn_id).await;

    sqlx::query(&format!("DROP TRIGGER {trigger_name} ON trace_payloads"))
        .execute(storage.pool())
        .await
        .unwrap();
    sqlx::query(&format!("DROP FUNCTION {function_name}()"))
        .execute(storage.pool())
        .await
        .unwrap();

    assert!(flush.flushed);
    assert!(flush.write_failures >= 1);
    let span_status: String = sqlx::query_scalar("SELECT status FROM trace_spans WHERE id = $1")
        .bind(&span_id)
        .fetch_one(storage.pool())
        .await
        .unwrap();
    assert_eq!(span_status, "succeeded");
    let payload_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM trace_span_payloads WHERE span_id = $1")
            .bind(&span_id)
            .fetch_one(storage.pool())
            .await
            .unwrap();
    assert_eq!(payload_count, 0);

    storage.delete_session(&session_id).await.unwrap();
}

#[tokio::test]
async fn deleting_a_session_sweeps_unique_payloads_but_restrict_preserves_shared_payloads() {
    let Some(storage) = storage().await else {
        return;
    };
    let first_session = create_session(&storage, "session-payload-delete-first").await;
    let second_session = create_session(&storage, "session-payload-delete-second").await;
    let recorder = PostgresTraceRecorder::spawn(storage.pool().clone());
    let shared_tools = json!([{"name": "shared", "description": "same", "parameters": {}}]);
    let mut spans = Vec::new();
    for (session_id, unique_body) in [
        (&first_session, "first session private source"),
        (&second_session, "second session private source"),
    ] {
        let trace_id = unique("trace-delete");
        let root = compaction(session_id, &trace_id, unique("span-compaction"));
        let root_id = root.span_id.clone();
        recorder.record(finish_compaction(root));
        let request = request(unique_body);
        let mut started = model_started(
            session_id,
            None,
            &trace_id,
            Some(root_id),
            unique("span-delete"),
            &request,
        );
        started.payloads.tool_definitions = Some(shared_tools.clone());
        spans.push(started.span_id.clone());
        recorder.record(finished_model(started, None));
    }
    assert!(recorder.flush_session(&first_session).await.flushed);

    let first_unique_hash: String = sqlx::query_scalar(
        "SELECT payload_hash FROM trace_span_payloads
         WHERE span_id = $1 AND slot = 'request'",
    )
    .bind(&spans[0])
    .fetch_one(storage.pool())
    .await
    .unwrap();
    let shared_hash: String = sqlx::query_scalar(
        "SELECT payload_hash FROM trace_span_payloads
         WHERE span_id = $1 AND slot = 'tool_definitions'",
    )
    .bind(&spans[0])
    .fetch_one(storage.pool())
    .await
    .unwrap();
    let second_shared_hash: String = sqlx::query_scalar(
        "SELECT payload_hash FROM trace_span_payloads
         WHERE span_id = $1 AND slot = 'tool_definitions'",
    )
    .bind(&spans[1])
    .fetch_one(storage.pool())
    .await
    .unwrap();
    assert_eq!(shared_hash, second_shared_hash);

    let restricted = sqlx::query("DELETE FROM trace_payloads WHERE hash = $1")
        .bind(&shared_hash)
        .execute(storage.pool())
        .await
        .expect_err("referenced payload must be protected");
    assert_eq!(
        restricted
            .as_database_error()
            .and_then(|error| error.code())
            .as_deref(),
        Some("23503")
    );

    storage.delete_session(&first_session).await.unwrap();
    let first_unique_exists: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM trace_payloads WHERE hash = $1)")
            .bind(&first_unique_hash)
            .fetch_one(storage.pool())
            .await
            .unwrap();
    let shared_exists: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM trace_payloads WHERE hash = $1)")
            .bind(&shared_hash)
            .fetch_one(storage.pool())
            .await
            .unwrap();
    assert!(!first_unique_exists);
    assert!(shared_exists);
    let remaining_reference: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM trace_span_payloads mapping
         JOIN trace_spans span ON span.id = mapping.span_id
         WHERE span.session_id = $1 AND mapping.payload_hash = $2",
    )
    .bind(second_session.as_str())
    .bind(&shared_hash)
    .fetch_one(storage.pool())
    .await
    .unwrap();
    assert_eq!(remaining_reference, 1);

    storage.delete_session(&second_session).await.unwrap();
}

#[tokio::test]
async fn retention_purges_expired_unannotated_payloads_and_preserves_shared_bodies() {
    let Some(storage) = storage().await else {
        return;
    };
    let session_id = create_session(&storage, "session-payload-retention").await;
    let recorder = PostgresTraceRecorder::spawn(storage.pool().clone());

    let expired_trace_id = unique("trace-expired");
    let expired_span_id = unique("span-expired");
    let expired_request = request(&unique("expired-request-body"));
    recorder.record(finished_model(
        model_started(
            &session_id,
            None,
            &expired_trace_id,
            None,
            expired_span_id.clone(),
            &expired_request,
        ),
        Some(json!({"text": unique("expired-response-body")})),
    ));

    let annotated_trace_id = unique("trace-annotated");
    let annotated_span_ids = [unique("span-annotated-a"), unique("span-annotated-b")];
    for (index, span_id) in annotated_span_ids.iter().enumerate() {
        let annotated_request = request(&format!("annotated-request-{index}"));
        recorder.record(finished_model(
            model_started(
                &session_id,
                None,
                &annotated_trace_id,
                None,
                span_id.clone(),
                &annotated_request,
            ),
            Some(json!({"text": format!("annotated-response-{index}")})),
        ));
    }

    let fresh_trace_id = unique("trace-fresh");
    let fresh_span_id = unique("span-fresh");
    let fresh_request = request(&unique("fresh-request-body"));
    recorder.record(finished_model(
        model_started(
            &session_id,
            None,
            &fresh_trace_id,
            None,
            fresh_span_id.clone(),
            &fresh_request,
        ),
        Some(json!({"text": unique("fresh-response-body")})),
    ));
    assert!(recorder.flush_session(&session_id).await.flushed);

    let expired_unique_hash: String = sqlx::query_scalar(
        "SELECT payload_hash FROM trace_span_payloads
         WHERE span_id = $1 AND slot = 'request'",
    )
    .bind(&expired_span_id)
    .fetch_one(storage.pool())
    .await
    .unwrap();
    let shared_hash: String = sqlx::query_scalar(
        "SELECT payload_hash FROM trace_span_payloads
         WHERE span_id = $1 AND slot = 'tool_definitions'",
    )
    .bind(&expired_span_id)
    .fetch_one(storage.pool())
    .await
    .unwrap();

    // Phase 4 will provide the normal annotation writer. This direct insert
    // pins the retention constraint before that write path exists.
    sqlx::query(
        "INSERT INTO trace_annotations (id, session_id, trace_id, span_id, rating)
         VALUES ($1, $2, $3, $4, 'bad')",
    )
    .bind(unique("annotation-retention"))
    .bind(session_id.as_str())
    .bind(&annotated_trace_id)
    .bind(&annotated_span_ids[0])
    .execute(storage.pool())
    .await
    .unwrap();
    sqlx::query(
        "UPDATE trace_spans
         SET started_at = (CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai') - INTERVAL '31 days'
         WHERE id IN ($1, $2, $3)",
    )
    .bind(&expired_span_id)
    .bind(&annotated_span_ids[0])
    .bind(&annotated_span_ids[1])
    .execute(storage.pool())
    .await
    .unwrap();
    drop(recorder);

    let deleted_mapping_count = storage.purge_expired_trace_payloads(30).await.unwrap();
    assert_eq!(deleted_mapping_count, 4);

    let expired_mapping_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM trace_span_payloads WHERE span_id = $1")
            .bind(&expired_span_id)
            .fetch_one(storage.pool())
            .await
            .unwrap();
    assert_eq!(expired_mapping_count, 0);
    let expired_span: (String, Option<i64>, Option<i64>) =
        sqlx::query_as("SELECT status, input_tokens, output_tokens FROM trace_spans WHERE id = $1")
            .bind(&expired_span_id)
            .fetch_one(storage.pool())
            .await
            .unwrap();
    assert_eq!(expired_span, ("succeeded".to_string(), Some(11), Some(7)));
    let expired_body_exists: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM trace_payloads WHERE hash = $1)")
            .bind(&expired_unique_hash)
            .fetch_one(storage.pool())
            .await
            .unwrap();
    assert!(!expired_body_exists);

    let annotated_mapping_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM trace_span_payloads
         WHERE span_id IN ($1, $2)",
    )
    .bind(&annotated_span_ids[0])
    .bind(&annotated_span_ids[1])
    .fetch_one(storage.pool())
    .await
    .unwrap();
    assert_eq!(annotated_mapping_count, 8);
    let fresh_mapping_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM trace_span_payloads WHERE span_id = $1")
            .bind(&fresh_span_id)
            .fetch_one(storage.pool())
            .await
            .unwrap();
    assert_eq!(fresh_mapping_count, 4);

    let shared_body_exists: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM trace_payloads WHERE hash = $1)")
            .bind(&shared_hash)
            .fetch_one(storage.pool())
            .await
            .unwrap();
    assert!(shared_body_exists);
    let restricted = sqlx::query("DELETE FROM trace_payloads WHERE hash = $1")
        .bind(&shared_hash)
        .execute(storage.pool())
        .await
        .expect_err("a body retained by fresh or annotated mappings must stay protected");
    assert_eq!(
        restricted
            .as_database_error()
            .and_then(|error| error.code())
            .as_deref(),
        Some("23503")
    );

    storage.delete_session(&session_id).await.unwrap();
}
