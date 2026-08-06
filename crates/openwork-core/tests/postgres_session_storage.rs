use openwork_chat_state::{ChatStateHandle, ConversationItemOrigin, SyntheticReason};
use openwork_core::{
    ClientRequestId, CompactionFinished, CompactionRuntimeState, CompactionStarted,
    CompactionStateCollector, CompactionTraceAttributesV1, ConversationCompactionKind,
    ConversationProjectionSelector, ConversationTranscriptQuery, ModelCallFinished,
    ModelCallStarted, ModelInput, ModelTraceAttributesV1, NewConversationCompaction,
    PostgresStorage, PostgresTraceRecorder, ResolvedModel, SessionId, SessionInput, SessionStorage,
    StoredMessageKind, ToolCallFinished, ToolCallStarted, ToolTraceAttributesV1, TracePayloads,
    TraceRecorder, TraceSignal, TraceSpanRecord, TraceStatus, TurnOutcome, session::TurnId,
};
use openwork_models::model::{
    ContentBlock, Message, Role, TokenUsage, ToolCallBlock, ToolCallState, ToolResultArtifact,
    ToolResultBlock, ToolResultState,
};
use serde_json::json;
use time::{OffsetDateTime, format_description::well_known::Rfc3339};
use uuid::Uuid;

fn test_database_url() -> Option<String> {
    std::env::var("TEST_DATABASE_URL").ok()
}

fn unique(prefix: &str) -> String {
    format!("{prefix}-{}", Uuid::new_v4().simple())
}

fn absolute_time(value: &str) -> OffsetDateTime {
    OffsetDateTime::parse(value, &Rfc3339).unwrap()
}

fn compaction_summary() -> String {
    let detail = "Preserve the durable raw transcript, the exact user request, the completed file-reading result, verified implementation state, concrete persistence boundaries, uncertainty, pending work, and the next safe action without inventing completion.";
    let headings = [
        "## 1. Primary Request and Intent",
        "## 2. Key Technical Concepts",
        "## 3. Files and Code Sections",
        "## 4. Errors and Fixes",
        "## 5. Problem Solving and Decisions",
        "## 6. User Messages and Constraints",
        "## 7. Pending Tasks",
        "## 8. Current Work",
        "## 9. Next Safe Action",
    ];
    let body = headings
        .into_iter()
        .map(|heading| format!("{heading}\n{detail}"))
        .collect::<Vec<_>>()
        .join("\n\n");
    format!("<conversation_summary format_version=\"1\">\n{body}\n</conversation_summary>")
}

type TurnUsageSummary = (
    String,
    i32,
    i32,
    i32,
    Option<i64>,
    Option<i64>,
    Option<i64>,
    Option<i64>,
    Option<i64>,
);

type TraceUsageRow = (
    String,
    String,
    Option<String>,
    Option<i64>,
    Option<i64>,
    Option<i64>,
    Option<i64>,
    Option<i64>,
);

type CompactionTraceShapeRow = (
    String,
    Option<String>,
    Option<String>,
    Option<i64>,
    Option<i64>,
);

#[test]
fn trace_span_contract_exposes_reasoning_and_total_tokens() {
    fn usage_fields(span: &TraceSpanRecord) -> (Option<i64>, Option<i64>) {
        (span.reasoning_tokens, span.total_tokens)
    }

    let _ = usage_fields;
}

#[tokio::test]
async fn postgres_trace_recorder_persists_a_session_scoped_compaction() {
    let Some(database_url) = test_database_url() else {
        return;
    };
    let storage = PostgresStorage::connect(Some(&database_url)).await.unwrap();
    storage.migrate().await.unwrap();
    let session_id = SessionId::new(unique("session-compaction-trace"));
    storage
        .create_session(&SessionInput {
            id: session_id.clone(),
            title: Some("Compaction trace".to_string()),
            working_directory: "/tmp/openwork-compaction-trace".to_string(),
            default_model_id: None,
        })
        .await
        .unwrap();

    let trace = PostgresTraceRecorder::spawn(storage.pool().clone());
    let started = CompactionStarted {
        span_id: unique("span-compaction"),
        trace_id: unique("trace-compaction"),
        session_id: session_id.clone(),
        turn_id: None,
        model_id: None,
        resolved_model_name: "test-model".to_string(),
        started_at: absolute_time("2026-07-25T08:30:45+08:00"),
        attributes: CompactionTraceAttributesV1::new("manual"),
    };
    trace.record(TraceSignal::CompactionStarted(Box::new(started.clone())));
    let summary_request = openwork_models::model::ModelRequest::text("test-model", "summarize");
    let summary_started = ModelCallStarted {
        span_id: unique("span-summary-model"),
        trace_id: started.trace_id.clone(),
        session_id: session_id.clone(),
        turn_id: None,
        parent_span_id: Some(started.span_id.clone()),
        model_id: None,
        resolved_model_name: "test-model".to_string(),
        started_at: absolute_time("2026-07-25T08:30:45.250+08:00"),
        attributes: ModelTraceAttributesV1::from_request(1, 4, &summary_request),
        payloads: TracePayloads::for_model_request(&summary_request),
    };
    trace.record(TraceSignal::ModelCallStarted(Box::new(
        summary_started.clone(),
    )));
    trace.record(TraceSignal::ModelCallFinished(Box::new(
        ModelCallFinished {
            started: summary_started.clone(),
            status: TraceStatus::Degenerate,
            provider_request_id: Some("summary-provider-request-1".to_string()),
            attempt_count: Some(1),
            usage: None,
            ended_at: absolute_time("2026-07-25T08:30:45.400+08:00"),
            error_code: Some("invalid_response".to_string()),
            error_message: Some("summary was unusable".to_string()),
            attributes: summary_started.attributes.clone(),
            response_message_id: None,
            response_payload: None,
        },
    )));
    let successful_summary_started = ModelCallStarted {
        span_id: unique("span-summary-model"),
        started_at: absolute_time("2026-07-25T08:30:45.500+08:00"),
        attributes: ModelTraceAttributesV1::from_request(2, 0, &summary_request),
        ..summary_started.clone()
    };
    trace.record(TraceSignal::ModelCallStarted(Box::new(
        successful_summary_started.clone(),
    )));
    trace.record(TraceSignal::ModelCallFinished(Box::new(
        ModelCallFinished {
            started: successful_summary_started.clone(),
            status: TraceStatus::Succeeded,
            provider_request_id: Some("summary-provider-request".to_string()),
            attempt_count: Some(1),
            usage: Some(TokenUsage {
                input_tokens: Some(30),
                output_tokens: Some(20),
                total_tokens: Some(50),
                cached_input_tokens: None,
                cache_creation_input_tokens: None,
                reasoning_tokens: None,
            }),
            ended_at: absolute_time("2026-07-25T08:30:46+08:00"),
            error_code: None,
            error_message: None,
            attributes: successful_summary_started.attributes.clone(),
            response_message_id: None,
            response_payload: None,
        },
    )));
    trace.record(TraceSignal::CompactionFinished(Box::new(
        CompactionFinished {
            started: started.clone(),
            status: TraceStatus::Succeeded,
            attempt_count: Some(2),
            input_tokens: Some(30),
            output_tokens: Some(20),
            ended_at: absolute_time("2026-07-25T08:30:47+08:00"),
            error_code: None,
            error_message: None,
            attributes: started.attributes.clone(),
        },
    )));
    let flush = trace.flush_session(&session_id).await;
    assert!(flush.flushed);
    assert_eq!(flush.write_failures, 0);

    let classified_status: String =
        sqlx::query_scalar("SELECT status FROM trace_spans WHERE id = $1")
            .bind(&summary_started.span_id)
            .fetch_one(storage.pool())
            .await
            .unwrap();
    assert_eq!(classified_status, "degenerate");

    let row: (
        String,
        Option<String>,
        String,
        String,
        Option<i64>,
        Option<i64>,
    ) = sqlx::query_as(
        "SELECT session_id, turn_id, kind, status, input_tokens, output_tokens
             FROM trace_spans WHERE id = $1",
    )
    .bind(&started.span_id)
    .fetch_one(storage.pool())
    .await
    .unwrap();
    assert_eq!(
        row,
        (
            session_id.to_string(),
            None,
            "compaction".to_string(),
            "succeeded".to_string(),
            Some(30),
            Some(20),
        )
    );

    // A manual compaction has no Turn, so the Session-scoped query is the only
    // way to read it back.
    let spans = storage
        .list_compaction_spans(&session_id, 10)
        .await
        .unwrap();
    assert_eq!(spans.len(), 1);
    assert_eq!(spans[0].id, started.span_id);
    assert_eq!(spans[0].turn_id, None);
    assert_eq!(spans[0].kind, "compaction");
    assert_eq!(spans[0].attributes["trigger"], "manual");

    let trace_rows: Vec<CompactionTraceShapeRow> = sqlx::query_as(
        "SELECT kind, turn_id, parent_span_id, input_tokens, output_tokens
             FROM trace_spans WHERE trace_id = $1 ORDER BY started_at, id",
    )
    .bind(&started.trace_id)
    .fetch_all(storage.pool())
    .await
    .unwrap();
    assert_eq!(trace_rows.len(), 3);
    assert_eq!(trace_rows[0].0, "compaction");
    assert_eq!(trace_rows[1].0, "model_call");
    assert_eq!(trace_rows[1].1, None);
    assert_eq!(trace_rows[1].2.as_deref(), Some(started.span_id.as_str()));
    assert_eq!((trace_rows[1].3, trace_rows[1].4), (None, None));
    assert_eq!(trace_rows[2].0, "model_call");
    assert_eq!((trace_rows[2].3, trace_rows[2].4), (Some(30), Some(20)));
}

/// 运行记录列表必须能看到手动压缩。
///
/// 它没有 Turn，若列表以 `turns` 为根就永远不可见——这正是引入 `trace_id` 之前的行为。
#[tokio::test]
async fn trace_list_includes_turnless_compaction_traces() {
    let Some(database_url) = test_database_url() else {
        return;
    };
    let storage = PostgresStorage::connect(Some(&database_url)).await.unwrap();
    storage.migrate().await.unwrap();
    let session_id = SessionId::new(unique("session-trace-list-compaction"));
    storage
        .create_session(&SessionInput {
            id: session_id.clone(),
            title: Some("Trace list".to_string()),
            working_directory: "/tmp/openwork-trace-list".to_string(),
            default_model_id: None,
        })
        .await
        .unwrap();

    let trace = PostgresTraceRecorder::spawn(storage.pool().clone());
    let started = CompactionStarted {
        span_id: unique("span-manual-compaction"),
        trace_id: unique("trace-manual"),
        session_id: session_id.clone(),
        turn_id: None,
        model_id: None,
        resolved_model_name: "test-model".to_string(),
        started_at: absolute_time("2026-07-26T09:00:00+08:00"),
        attributes: CompactionTraceAttributesV1::new("manual"),
    };
    trace.record(TraceSignal::CompactionFinished(Box::new(
        CompactionFinished {
            started: started.clone(),
            status: TraceStatus::Succeeded,
            attempt_count: Some(1),
            input_tokens: Some(30),
            output_tokens: Some(20),
            ended_at: absolute_time("2026-07-26T09:00:02+08:00"),
            error_code: None,
            error_message: None,
            attributes: started.attributes.clone(),
        },
    )));
    assert!(trace.flush_session(&session_id).await.flushed);

    let listed = storage.list_traces(Some(&session_id), 100).await.unwrap();
    let row = listed
        .iter()
        .find(|summary| summary.trace_id == started.trace_id)
        .expect("手动压缩必须出现在运行记录列表里");

    assert_eq!(row.turn_id, None);
    assert_eq!(row.turn_sequence, None);
    // Span 的 `succeeded` 映射成 Turn 的 `completed`，列表只有一套状态词汇。
    assert_eq!(row.status, "completed");
    assert_eq!(row.span_count, 1);
    assert_eq!(row.model_call_count, 0);
    // 压缩 span 记录了 30 input + 20 output，独立 Trace 的 token 合计同样来自 span 实测。
    assert_eq!(row.total_tokens, 50);
    assert!(row.started_at.ends_with("+08:00"), "{}", row.started_at);
}

#[tokio::test]
async fn session_compaction_query_orders_newest_first_and_excludes_other_kinds() {
    let Some(database_url) = test_database_url() else {
        return;
    };
    let storage = PostgresStorage::connect(Some(&database_url)).await.unwrap();
    storage.migrate().await.unwrap();
    let session_id = SessionId::new(unique("session-compaction-order"));
    storage
        .create_session(&SessionInput {
            id: session_id.clone(),
            title: Some("Compaction order".to_string()),
            working_directory: "/tmp/openwork-compaction-order".to_string(),
            default_model_id: None,
        })
        .await
        .unwrap();

    let trace = PostgresTraceRecorder::spawn(storage.pool().clone());
    let mut span_ids = Vec::new();
    for (index, started_at) in [
        "2026-07-25T08:00:00+08:00",
        "2026-07-25T09:00:00+08:00",
        "2026-07-25T10:00:00+08:00",
    ]
    .into_iter()
    .enumerate()
    {
        let started = CompactionStarted {
            span_id: unique(&format!("span-compaction-{index}")),
            trace_id: unique(&format!("trace-compaction-{index}")),
            session_id: session_id.clone(),
            turn_id: None,
            model_id: None,
            resolved_model_name: "test-model".to_string(),
            started_at: absolute_time(started_at),
            attributes: CompactionTraceAttributesV1::new("manual"),
        };
        span_ids.push(started.span_id.clone());
        trace.record(TraceSignal::CompactionFinished(Box::new(
            CompactionFinished {
                started: started.clone(),
                status: TraceStatus::Succeeded,
                attempt_count: Some(1),
                input_tokens: None,
                output_tokens: None,
                ended_at: absolute_time(started_at),
                error_code: None,
                error_message: None,
                attributes: started.attributes.clone(),
            },
        )));
    }
    assert!(trace.flush_session(&session_id).await.flushed);

    let spans = storage
        .list_compaction_spans(&session_id, 10)
        .await
        .unwrap();
    assert_eq!(
        spans
            .iter()
            .map(|span| span.id.as_str())
            .collect::<Vec<_>>(),
        vec![
            span_ids[2].as_str(),
            span_ids[1].as_str(),
            span_ids[0].as_str(),
        ]
    );

    let limited = storage.list_compaction_spans(&session_id, 2).await.unwrap();
    assert_eq!(limited.len(), 2);
    assert_eq!(limited[0].id, span_ids[2]);
}

#[tokio::test]
async fn postgres_storage_round_trips_a_threshold_compaction_for_an_active_turn() {
    let Some(database_url) = test_database_url() else {
        return;
    };
    let storage = PostgresStorage::connect(Some(&database_url)).await.unwrap();
    storage.migrate().await.unwrap();

    let model_id = unique("model-threshold");
    storage
        .upsert_model(&ModelInput {
            id: model_id.clone(),
            display_name: "Threshold test model".to_string(),
            provider_kind: "deepseek".to_string(),
            model_name: "threshold-test-model".to_string(),
            base_url: format!("https://example.invalid/threshold/{model_id}"),
            credential_ref: Some("DEEPSEEK_API_KEY".to_string()),
            enabled: true,
            config: json!({}),
        })
        .await
        .unwrap();
    let session_id = SessionId::new(unique("session-threshold"));
    storage
        .create_session(&SessionInput {
            id: session_id.clone(),
            title: Some("Threshold compaction".to_string()),
            working_directory: "/tmp/openwork-threshold-test".to_string(),
            default_model_id: Some(model_id),
        })
        .await
        .unwrap();
    let turn_id = TurnId::new(unique("turn-threshold"));
    storage
        .begin_turn(
            &session_id,
            &turn_id,
            &ClientRequestId::new(unique("request-threshold")),
            &ResolvedModel::new(None::<String>, "deepseek", "threshold-test-model"),
            &[],
            &Message::text(Role::User, "continue the task"),
        )
        .await
        .unwrap();

    let checkpoint = storage
        .save_conversation_compaction(
            &session_id,
            NewConversationCompaction {
                kind: ConversationCompactionKind::Threshold,
                source_message_count: 1,
                resolved_model_name: "threshold-test-model".to_string(),
                summary: compaction_summary(),
                runtime_state: CompactionRuntimeState::default(),
                runtime_reminder: "<system_reminder format_version=\"1\">\nNo additional durable runtime state was recorded at compaction time.\n</system_reminder>".to_string(),
                input_tokens: Some(219_300),
                output_tokens: Some(800),
                trigger_turn_id: Some(turn_id.clone()),
                last_user_message_id: None,
                last_user_message_sequence: None,
            },
        )
        .await
        .unwrap();

    assert_eq!(checkpoint.kind, ConversationCompactionKind::Threshold);
    assert_eq!(
        checkpoint.trigger_turn_id.as_deref(),
        Some(turn_id.as_str())
    );
    storage
        .finish_turn(
            &turn_id,
            &TurnOutcome::Completed {
                final_text: "done".to_string(),
            },
        )
        .await
        .unwrap();
}

#[tokio::test]
async fn postgres_persists_contextual_input_before_the_visible_user_message() {
    let Some(database_url) = test_database_url() else {
        return;
    };
    let storage = PostgresStorage::connect(Some(&database_url)).await.unwrap();
    storage.migrate().await.unwrap();
    let session_id = SessionId::new(unique("session-contextual-input"));
    storage
        .create_session(&SessionInput {
            id: session_id.clone(),
            title: Some("Contextual input persistence".to_string()),
            working_directory: "/tmp/openwork-contextual-input".to_string(),
            default_model_id: None,
        })
        .await
        .unwrap();
    let turn_id = TurnId::new(unique("turn-contextual-input"));
    storage
        .begin_turn(
            &session_id,
            &turn_id,
            &ClientRequestId::new(unique("request-contextual-input")),
            &ResolvedModel::new(None::<String>, "deepseek", "contextual-input-test"),
            &[Message::text(
                Role::User,
                "<skill>\n<name>read-workflow</name>\n<path>/tmp/read-workflow/SKILL.md</path>\nprivate skill body\n</skill>",
            )],
            &Message::text(Role::User, "read the file"),
        )
        .await
        .unwrap();

    let records = storage.load_message_records(&session_id).await.unwrap();
    assert_eq!(records.len(), 2);
    assert_eq!(records[0].sequence, 1);
    assert_eq!(records[0].message_kind, StoredMessageKind::SkillInstruction);
    assert!(matches!(
        &records[0].content[0],
        ContentBlock::Text(block) if block.text.contains("private skill body")
    ));
    assert_eq!(records[1].sequence, 2);
    assert_eq!(records[1].message_kind, StoredMessageKind::Normal);
    assert!(matches!(
        &records[1].content[0],
        ContentBlock::Text(block) if block.text == "read the file"
    ));
    storage
        .finish_turn(
            &turn_id,
            &TurnOutcome::Completed {
                final_text: "stored".to_string(),
            },
        )
        .await
        .unwrap();
}

#[tokio::test]
async fn postgres_storage_round_trips_a_complete_tool_turn() {
    let Some(database_url) = test_database_url() else {
        return;
    };
    let storage = PostgresStorage::connect(Some(&database_url)).await.unwrap();
    storage.migrate().await.unwrap();
    storage.migrate().await.unwrap();

    let sqlx_migrations: Option<String> =
        sqlx::query_scalar("SELECT to_regclass('public._sqlx_migrations')::text")
            .fetch_one(storage.pool())
            .await
            .unwrap();
    let legacy_migrations: Option<String> =
        sqlx::query_scalar("SELECT to_regclass('public.schema_migrations')::text")
            .fetch_one(storage.pool())
            .await
            .unwrap();
    assert_eq!(sqlx_migrations.as_deref(), Some("_sqlx_migrations"));
    assert_eq!(legacy_migrations, None);
    let business_tables: Vec<String> = sqlx::query_scalar(
        "SELECT tablename
         FROM pg_tables
         WHERE schemaname = 'public' AND tablename <> '_sqlx_migrations'
         ORDER BY tablename",
    )
    .fetch_all(storage.pool())
    .await
    .unwrap();
    assert_eq!(
        business_tables,
        vec![
            "conversation_compactions",
            "messages",
            "models",
            "provider_credentials",
            "sessions",
            "skill_status",
            "trace_annotations",
            "trace_payloads",
            "trace_span_payloads",
            "trace_spans",
            "turns",
        ]
    );
    let timestamp_columns: Vec<(String, String, String)> = sqlx::query_as(
        "SELECT table_name, column_name, data_type
         FROM information_schema.columns
         WHERE table_schema = 'public'
           AND table_name = ANY($1)
           AND data_type LIKE 'timestamp%'
         ORDER BY table_name, ordinal_position",
    )
    .bind(&business_tables)
    .fetch_all(storage.pool())
    .await
    .unwrap();
    assert_eq!(timestamp_columns.len(), 17);
    assert!(
        timestamp_columns
            .iter()
            .all(|(_, _, data_type)| data_type == "timestamp without time zone")
    );
    let applied_migrations: Vec<(i64, String, bool)> = sqlx::query_as(
        "SELECT version, description, success FROM _sqlx_migrations ORDER BY version",
    )
    .fetch_all(storage.pool())
    .await
    .unwrap();
    assert_eq!(
        applied_migrations,
        vec![
            (202_607_260_001, "initial schema".to_string(), true),
            (202_608_040_001, "create skill status".to_string(), true),
            (202_608_050_001, "add message kind".to_string(), true),
        ]
    );

    let model_id = unique("model-test");
    storage
        .upsert_model(&ModelInput {
            id: model_id.clone(),
            display_name: "Postgres test model".to_string(),
            provider_kind: "deepseek".to_string(),
            model_name: "deepseek-v4-flash".to_string(),
            base_url: format!("https://example.invalid/{model_id}"),
            credential_ref: Some("DEEPSEEK_API_KEY".to_string()),
            enabled: true,
            config: json!({}),
        })
        .await
        .unwrap();

    let session_id = SessionId::new(unique("session-test"));
    let session = storage
        .create_session(&SessionInput {
            id: session_id.clone(),
            title: Some("Postgres storage test".to_string()),
            working_directory: "/tmp/openwork-postgres-test".to_string(),
            default_model_id: Some(model_id.clone()),
        })
        .await
        .unwrap();
    assert!(session.created_at.ends_with("+08:00"));
    assert!(session.updated_at.ends_with("+08:00"));

    let turn_id = TurnId::new(unique("turn-test"));
    let client_request_id = ClientRequestId::new(unique("request-test"));
    storage
        .begin_turn(
            &session_id,
            &turn_id,
            &client_request_id,
            &ResolvedModel::new(Some(model_id.clone()), "deepseek", "deepseek-v4-flash"),
            &[],
            &Message::text(Role::User, "read the file"),
        )
        .await
        .unwrap();
    let stored_app_version: String =
        sqlx::query_scalar("SELECT app_version FROM turns WHERE id = $1")
            .bind(turn_id.as_str())
            .fetch_one(storage.pool())
            .await
            .unwrap();
    assert_eq!(stored_app_version, env!("CARGO_PKG_VERSION"));
    storage.begin_model_call(&turn_id, 1, 1).await.unwrap();
    storage
        .append_assistant_message(
            &turn_id,
            &Message {
                role: Role::Assistant,
                content: vec![ContentBlock::ToolCall(ToolCallBlock {
                    id: "provider-call-1".to_string(),
                    name: "read_file".to_string(),
                    input: r#"{"path":"README.md"}"#.to_string(),
                    state: ToolCallState::Submitted,
                })],
            },
            Some(TokenUsage {
                input_tokens: Some(11),
                output_tokens: Some(7),
                total_tokens: Some(18),
                cached_input_tokens: Some(3),
                cache_creation_input_tokens: None,
                reasoning_tokens: Some(5),
            }),
        )
        .await
        .unwrap();
    storage
        .append_tool_result(
            &turn_id,
            &Message {
                role: Role::Tool,
                content: vec![ContentBlock::ToolResult(ToolResultBlock {
                    id: "provider-call-1".to_string(),
                    name: "read_file".to_string(),
                    output: vec![ContentBlock::text("file contents")],
                    state: ToolResultState::Success,
                    artifacts: vec![ToolResultArtifact {
                        kind: "file_change".to_string(),
                        payload: json!({
                            "changeId": "change-storage",
                            "path": "README.md",
                            "kind": "modified",
                            "additions": 1,
                            "deletions": 1,
                            "hunks": [],
                            "beforeHash": "before",
                            "afterHash": "after",
                            "undone": false
                        }),
                    }],
                })],
            },
        )
        .await
        .unwrap();
    storage.begin_model_call(&turn_id, 2, 1).await.unwrap();
    storage.begin_model_call(&turn_id, 2, 2).await.unwrap();
    storage
        .append_assistant_message(
            &turn_id,
            &Message::text(Role::Assistant, "done"),
            Some(TokenUsage {
                input_tokens: Some(19),
                output_tokens: Some(2),
                total_tokens: Some(21),
                cached_input_tokens: None,
                cache_creation_input_tokens: None,
                reasoning_tokens: Some(1),
            }),
        )
        .await
        .unwrap();
    storage
        .finish_turn(
            &turn_id,
            &TurnOutcome::Completed {
                final_text: "done".to_string(),
            },
        )
        .await
        .unwrap();

    let messages = storage.load_messages(&session_id).await.unwrap();
    assert_eq!(
        messages
            .iter()
            .map(|message| message.role)
            .collect::<Vec<_>>(),
        vec![Role::User, Role::Assistant, Role::Tool, Role::Assistant]
    );
    let compacted = storage
        .save_conversation_compaction(
            &session_id,
            NewConversationCompaction {
                kind: ConversationCompactionKind::Manual,
                source_message_count: 4,
                resolved_model_name: "deepseek-v4-flash".to_string(),
                summary: compaction_summary(),
                runtime_state: CompactionRuntimeState::default(),
                runtime_reminder: "<system_reminder format_version=\"1\">\nNo additional durable runtime state was recorded at compaction time.\n</system_reminder>".to_string(),
                input_tokens: Some(30),
                output_tokens: Some(9),
                trigger_turn_id: None,
                last_user_message_id: None,
                last_user_message_sequence: None,
            },
        )
        .await
        .unwrap();
    assert_eq!(compacted.through_message_sequence, 4);
    assert_eq!(compacted.input_tokens, Some(30));
    let projected = storage.load_messages(&session_id).await.unwrap();
    assert_eq!(projected.len(), 3);
    assert_eq!(projected[0].role, Role::User);
    let ContentBlock::Text(projected_summary) = &projected[0].content[0] else {
        panic!("projected compaction summary")
    };
    assert_eq!(projected_summary.text, "read the file");
    let ContentBlock::Text(projected_summary) = &projected[1].content[0] else {
        panic!("projected compaction summary")
    };
    assert!(
        projected_summary
            .text
            .contains("The earlier Conversation was compacted")
    );
    let mut records = storage.load_message_records(&session_id).await.unwrap();
    assert_eq!(records.len(), 4);
    let tool_record = records
        .iter_mut()
        .find(|record| record.role == Role::Tool)
        .expect("stored tool result");
    let ContentBlock::ToolResult(result) = &mut tool_record.content[0] else {
        panic!("tool result block");
    };
    result.artifacts[0].payload["undone"] = json!(true);
    storage
        .replace_message_contents(
            &session_id,
            &[(tool_record.id.clone(), tool_record.content.clone())],
        )
        .await
        .unwrap();
    let reloaded = storage.load_message_records(&session_id).await.unwrap();
    let undone = reloaded
        .iter()
        .flat_map(|record| &record.content)
        .find_map(|block| match block {
            ContentBlock::ToolResult(result) => result.artifacts.first(),
            _ => None,
        })
        .and_then(|artifact| artifact.payload["undone"].as_bool());
    assert_eq!(undone, Some(true));
    let state_messages = storage
        .load_compaction_source_messages(&session_id)
        .await
        .unwrap();
    let (runtime_state_after_undo, _) = CompactionStateCollector::default()
        .collect_with_base(&state_messages, compacted.runtime_state.clone())
        .await
        .unwrap();
    assert!(runtime_state_after_undo.edited_paths.is_empty());

    let continuation_turn_id = TurnId::new(unique("turn-after-compaction"));
    storage
        .begin_turn(
            &session_id,
            &continuation_turn_id,
            &ClientRequestId::new(unique("request-after-compaction")),
            &ResolvedModel::new(Some(model_id.clone()), "deepseek", "deepseek-v4-flash"),
            &[],
            &Message::text(Role::User, "continue from the summary"),
        )
        .await
        .unwrap();
    storage
        .append_assistant_message(
            &continuation_turn_id,
            &Message::text(Role::Assistant, "continued"),
            None,
        )
        .await
        .unwrap();
    storage
        .finish_turn(
            &continuation_turn_id,
            &TurnOutcome::Completed {
                final_text: "continued".to_string(),
            },
        )
        .await
        .unwrap();
    let projected_with_tail = storage.load_messages(&session_id).await.unwrap();
    assert_eq!(
        projected_with_tail
            .iter()
            .map(|message| message.role)
            .collect::<Vec<_>>(),
        vec![
            Role::User,
            Role::User,
            Role::User,
            Role::User,
            Role::Assistant,
        ]
    );
    assert_eq!(
        storage
            .load_message_records(&session_id)
            .await
            .unwrap()
            .len(),
        6
    );
    let projected_request = openwork_models::model::ModelRequest {
        model: "deepseek-v4-flash".to_string(),
        messages: projected_with_tail.clone(),
        temperature: None,
        top_p: None,
        max_output_tokens: None,
        thinking: None,
        tools: Vec::new(),
    };
    let projected_payload = TracePayloads::for_model_request(&projected_request)
        .request
        .expect("projected request payload");
    assert_eq!(
        projected_payload,
        serde_json::to_value(&projected_with_tail).unwrap()
    );
    let projected_payload = serde_json::to_string(&projected_payload).unwrap();
    assert!(projected_payload.contains("The earlier Conversation was compacted"));
    assert!(projected_payload.contains("continue from the summary"));
    assert!(!projected_payload.contains("provider-call-1"));
    assert!(!projected_payload.contains("file contents"));

    let restarted_items = storage.load_conversation_items(&session_id).await.unwrap();
    assert_eq!(restarted_items.len(), 5);
    assert!(matches!(
        restarted_items[0].origin,
        ConversationItemOrigin::Synthetic {
            reason: SyntheticReason::LastUserRequestReplay,
            ..
        }
    ));
    assert!(matches!(
        restarted_items[1].origin,
        ConversationItemOrigin::Synthetic {
            reason: SyntheticReason::CompactionSummary,
            ..
        }
    ));
    assert!(matches!(
        restarted_items[2].origin,
        ConversationItemOrigin::Synthetic {
            reason: SyntheticReason::SystemReminder,
            ..
        }
    ));
    assert!(restarted_items[3..].iter().all(|item| item.is_real()));
    let restarted_chat = ChatStateHandle::spawn_items(restarted_items).expect("restarted chat");
    assert_eq!(
        restarted_chat
            .conversation_view()
            .await
            .expect("restarted view")
            .messages
            .len(),
        5
    );

    let checkpoint_replay = storage
        .replay_conversation(
            &session_id,
            ConversationProjectionSelector::Compaction {
                compaction_id: compacted.id.clone(),
            },
        )
        .await
        .unwrap();
    assert_eq!(checkpoint_replay.messages.len(), 3);
    assert_eq!(
        checkpoint_replay.messages[0].message_kind,
        openwork_core::StoredMessageKind::Normal
    );
    assert_eq!(
        checkpoint_replay.checkpoint_id.as_deref(),
        Some(compacted.id.as_str())
    );

    let historical_replay = storage
        .replay_conversation(
            &session_id,
            ConversationProjectionSelector::ThroughMessage { sequence: 6 },
        )
        .await
        .unwrap();
    assert_eq!(historical_replay.messages.len(), 5);

    let raw_page_one = storage
        .read_compaction_transcript(
            &session_id,
            ConversationTranscriptQuery {
                compaction_id: Some(compacted.id.clone()),
                after_sequence: None,
                limit: Some(2),
            },
        )
        .await
        .unwrap();
    assert_eq!(raw_page_one.through_message_sequence, 4);
    assert_eq!(
        raw_page_one
            .messages
            .iter()
            .map(|message| message.sequence)
            .collect::<Vec<_>>(),
        [1, 2]
    );
    assert!(raw_page_one.has_more);
    assert_eq!(raw_page_one.next_after_sequence, Some(2));
    let raw_page_two = storage
        .read_compaction_transcript(
            &session_id,
            ConversationTranscriptQuery {
                compaction_id: Some(compacted.id.clone()),
                after_sequence: raw_page_one.next_after_sequence,
                limit: Some(2),
            },
        )
        .await
        .unwrap();
    assert_eq!(
        raw_page_two
            .messages
            .iter()
            .map(|message| message.sequence)
            .collect::<Vec<_>>(),
        [3, 4]
    );
    assert!(!raw_page_two.has_more);
    assert_eq!(raw_page_two.next_after_sequence, None);

    let rewound = storage
        .rewind_conversation_compaction(
            &session_id,
            &compacted.id,
            CompactionRuntimeState::default(),
            "<system_reminder format_version=\"1\">\nNo additional durable runtime state was recorded at rewind time.\n</system_reminder>"
                .to_string(),
        )
        .await
        .unwrap();
    assert_eq!(rewound.kind, ConversationCompactionKind::Rewind);
    assert_eq!(
        rewound.parent_compaction_id.as_deref(),
        Some(compacted.id.as_str())
    );
    assert_eq!(
        rewound.through_message_sequence,
        compacted.through_message_sequence
    );
    assert_eq!(rewound.replaced_through_message_sequence, 6);

    let latest_after_rewind = storage.load_messages(&session_id).await.unwrap();
    assert_eq!(latest_after_rewind.len(), 3);
    assert!(
        latest_after_rewind
            .iter()
            .all(|message| message.role == Role::User)
    );
    let checkpoints = storage
        .list_conversation_compactions(&session_id)
        .await
        .unwrap();
    assert_eq!(checkpoints.len(), 2);
    assert_eq!(checkpoints[0].id, rewound.id);
    assert_eq!(checkpoints[1].id, compacted.id);
    let latest_replay = storage
        .replay_conversation(&session_id, ConversationProjectionSelector::Latest)
        .await
        .unwrap();
    assert_eq!(
        latest_replay.checkpoint_id.as_deref(),
        Some(rewound.id.as_str())
    );
    assert_eq!(latest_replay.messages.len(), 3);
    let before_rewind_boundary = storage
        .replay_conversation(
            &session_id,
            ConversationProjectionSelector::ThroughMessage { sequence: 5 },
        )
        .await
        .unwrap();
    assert_eq!(
        before_rewind_boundary.checkpoint_id.as_deref(),
        Some(compacted.id.as_str())
    );
    assert_eq!(before_rewind_boundary.messages.len(), 4);
    let at_rewind_boundary = storage
        .replay_conversation(
            &session_id,
            ConversationProjectionSelector::ThroughMessage { sequence: 6 },
        )
        .await
        .unwrap();
    assert_eq!(
        at_rewind_boundary.checkpoint_id.as_deref(),
        Some(rewound.id.as_str())
    );
    assert_eq!(at_rewind_boundary.messages.len(), 3);
    let latest_raw_after_rewind = storage
        .read_compaction_transcript(&session_id, ConversationTranscriptQuery::default())
        .await
        .unwrap();
    assert_eq!(latest_raw_after_rewind.compaction_id, rewound.id);
    assert_eq!(latest_raw_after_rewind.through_message_sequence, 4);
    assert_eq!(latest_raw_after_rewind.messages.len(), 4);
    assert!(
        latest_raw_after_rewind
            .messages
            .iter()
            .all(|message| message.sequence <= 4)
    );

    let summary: TurnUsageSummary = sqlx::query_as(
        "SELECT status, model_call_count, model_submission_count, tool_call_count,
                input_tokens, output_tokens, cached_input_tokens,
                reasoning_tokens, (input_tokens + output_tokens) AS total_tokens
         FROM turns WHERE id = $1",
    )
    .bind(turn_id.as_str())
    .fetch_one(storage.pool())
    .await
    .unwrap();
    assert_eq!(
        summary,
        (
            "completed".to_string(),
            2,
            3,
            1,
            Some(30),
            Some(9),
            Some(3),
            Some(6),
            Some(39),
        )
    );

    let trace = PostgresTraceRecorder::spawn(storage.pool().clone());
    let model_span = ModelCallStarted {
        span_id: unique("span-model"),
        trace_id: turn_id.to_string(),
        session_id: session_id.clone(),
        turn_id: Some(turn_id.clone()),
        parent_span_id: None,
        model_id: Some(model_id.clone()),
        resolved_model_name: "deepseek-v4-flash".to_string(),
        started_at: absolute_time("2026-07-18T08:30:45+08:00"),
        attributes: ModelTraceAttributesV1::from_request(
            1,
            3,
            &openwork_models::model::ModelRequest::text("deepseek-v4-flash", "hello"),
        ),
        payloads: TracePayloads::for_model_request(&openwork_models::model::ModelRequest::text(
            "deepseek-v4-flash",
            "hello",
        )),
    };
    trace.record(TraceSignal::ModelCallStarted(Box::new(model_span.clone())));
    let mut model_finished_attributes = model_span.attributes.clone();
    model_finished_attributes.finish_reason = Some("stop".to_string());
    trace.record(TraceSignal::ModelCallFinished(Box::new(
        ModelCallFinished {
            started: model_span.clone(),
            status: TraceStatus::Succeeded,
            provider_request_id: Some("provider-request-test".to_string()),
            attempt_count: Some(1),
            usage: Some(TokenUsage {
                input_tokens: Some(11),
                output_tokens: Some(7),
                total_tokens: Some(18),
                cached_input_tokens: Some(3),
                cache_creation_input_tokens: None,
                reasoning_tokens: Some(5),
            }),
            ended_at: absolute_time("2026-07-18T08:30:47+08:00"),
            error_code: None,
            error_message: None,
            attributes: model_finished_attributes,
            response_message_id: None,
            response_payload: None,
        },
    )));
    let tool_span = ToolCallStarted {
        span_id: unique("span-tool"),
        trace_id: turn_id.to_string(),
        turn_id: turn_id.clone(),
        parent_span_id: model_span.span_id.clone(),
        provider_call_id: "provider-call-1".to_string(),
        requested_tool_name: "read_file".to_string(),
        started_at: absolute_time("2026-07-18T08:30:45.500+08:00"),
        attributes: ToolTraceAttributesV1::new(),
    };
    trace.record(TraceSignal::ToolCallStarted(Box::new(tool_span.clone())));
    let mut tool_finished_attributes = tool_span.attributes.clone();
    tool_finished_attributes.result_persisted = Some(true);
    trace.record(TraceSignal::ToolCallFinished(Box::new(ToolCallFinished {
        started: tool_span.clone(),
        status: TraceStatus::Succeeded,
        resolved_tool_name: Some("read_file".to_string()),
        permission_wait_ms: Some(4),
        ended_at: absolute_time("2026-07-18T08:30:46+08:00"),
        error_code: None,
        error_message: None,
        attributes: tool_finished_attributes,
        response_payload: None,
    })));
    let flush = trace.flush_turn(&turn_id).await;
    assert!(flush.flushed);
    assert_eq!(flush.write_failures, 0);
    let trace_rows: Vec<TraceUsageRow> = sqlx::query_as(
        "SELECT kind, status, parent_span_id,
                input_tokens, output_tokens, cached_input_tokens,
                reasoning_tokens, (input_tokens + output_tokens) AS total_tokens
         FROM trace_spans WHERE turn_id = $1 ORDER BY started_at, id",
    )
    .bind(turn_id.as_str())
    .fetch_all(storage.pool())
    .await
    .unwrap();
    assert_eq!(
        trace_rows,
        vec![
            (
                "model_call".to_string(),
                "succeeded".to_string(),
                None,
                Some(11),
                Some(7),
                Some(3),
                Some(5),
                Some(18),
            ),
            (
                "tool_call".to_string(),
                "succeeded".to_string(),
                Some(model_span.span_id),
                None,
                None,
                None,
                None,
                None,
            ),
        ]
    );
    let loaded_trace = storage.get_trace(&turn_id).await.unwrap();
    assert_eq!(
        loaded_trace.spans[0].started_at,
        "2026-07-18T08:30:45.000000+08:00"
    );
    assert_eq!(
        loaded_trace.spans[0].ended_at.as_deref(),
        Some("2026-07-18T08:30:47.000000+08:00")
    );
    assert_eq!(
        loaded_trace.spans[1].started_at,
        "2026-07-18T08:30:45.500000+08:00"
    );
    assert_eq!(
        loaded_trace.spans[1].ended_at.as_deref(),
        Some("2026-07-18T08:30:46.000000+08:00")
    );
    assert_eq!(loaded_trace.spans[0].attributes["schemaVersion"], 1);
    assert_eq!(loaded_trace.spans[0].attributes["finishReason"], "stop");
    assert_eq!(loaded_trace.spans[1].attributes["resultPersisted"], true);
    assert_eq!(loaded_trace.summary.trace_id, turn_id.as_str());
    assert_eq!(loaded_trace.summary.model_call_count, 2);
    assert_eq!(loaded_trace.summary.model_submission_count, 3);
    assert_eq!(loaded_trace.completeness.expected_model_calls, 3);
    assert_eq!(
        loaded_trace.completeness.state,
        openwork_core::TraceCompletenessState::Partial
    );
    let listed_trace = storage
        .list_traces(Some(&session_id), 100)
        .await
        .unwrap()
        .into_iter()
        .find(|summary| summary.turn_id.as_deref() == Some(turn_id.as_str()))
        .expect("turn trace summary");
    assert_eq!(listed_trace.trace_id, turn_id.as_str());
    assert_eq!(listed_trace.span_count, 2);
    // 模型 span 记录 11 + 7，工具 span 未记录 token，合计以实际加载的 span 为准。
    assert_eq!(listed_trace.total_tokens, 18);
    assert_eq!(loaded_trace.summary.total_tokens, 18);

    let interrupted_turn_id = TurnId::new(unique("turn-interrupted"));
    storage
        .begin_turn(
            &session_id,
            &interrupted_turn_id,
            &ClientRequestId::new(unique("request-interrupted")),
            &ResolvedModel::new(Some(model_id.clone()), "deepseek", "deepseek-v4-flash"),
            &[],
            &Message::text(Role::User, "this turn will be interrupted"),
        )
        .await
        .unwrap();
    assert_eq!(storage.mark_running_interrupted().await.unwrap(), 1);
    let interrupted_status: String = sqlx::query_scalar("SELECT status FROM turns WHERE id = $1")
        .bind(interrupted_turn_id.as_str())
        .fetch_one(storage.pool())
        .await
        .unwrap();
    assert_eq!(interrupted_status, "interrupted");

    storage.delete_session(&session_id).await.unwrap();
    sqlx::query("DELETE FROM models WHERE id = $1")
        .bind(&model_id)
        .execute(storage.pool())
        .await
        .unwrap();
}
