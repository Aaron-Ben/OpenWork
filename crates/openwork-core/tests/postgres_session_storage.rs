use openwork_core::{
    ClientRequestId, ModelCallFinished, ModelCallStarted, ModelInput, PostgresStorage,
    PostgresTraceRecorder, ResolvedModel, SessionId, SessionInput, SessionStorage,
    ToolCallFinished, ToolCallStarted, TraceRecorder, TraceSignal, TraceStatus, TurnOutcome,
    session::TurnId,
};
use openwork_models::model::{
    ContentBlock, Message, Role, TokenUsage, ToolCallBlock, ToolCallState, ToolResultBlock,
    ToolResultState,
};
use serde_json::json;
use time::OffsetDateTime;
use uuid::Uuid;

fn test_database_url() -> Option<String> {
    std::env::var("TEST_DATABASE_URL").ok()
}

fn unique(prefix: &str) -> String {
    format!("{prefix}-{}", Uuid::new_v4().simple())
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
            "messages",
            "models",
            "provider_credentials",
            "sessions",
            "trace_spans",
            "turns",
        ]
    );
    let applied_migrations: Vec<(i64, String, bool)> = sqlx::query_as(
        "SELECT version, description, success FROM _sqlx_migrations ORDER BY version",
    )
    .fetch_all(storage.pool())
    .await
    .unwrap();
    assert_eq!(
        applied_migrations,
        vec![(202_607_180_001, "initial schema".to_string(), true)]
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
    storage
        .create_session(&SessionInput {
            id: session_id.clone(),
            title: Some("Postgres storage test".to_string()),
            working_directory: "/tmp/openwork-postgres-test".to_string(),
            default_model_id: Some(model_id.clone()),
        })
        .await
        .unwrap();

    let turn_id = TurnId::new(unique("turn-test"));
    let client_request_id = ClientRequestId::new(unique("request-test"));
    storage
        .begin_turn(
            &session_id,
            &turn_id,
            &client_request_id,
            &ResolvedModel::new(Some(model_id.clone()), "deepseek", "deepseek-v4-flash"),
            &Message::text(Role::User, "read the file"),
        )
        .await
        .unwrap();
    storage.begin_model_call(&turn_id, 1).await.unwrap();
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
                reasoning_tokens: None,
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
                })],
            },
        )
        .await
        .unwrap();
    storage.begin_model_call(&turn_id, 2).await.unwrap();
    storage
        .append_assistant_message(
            &turn_id,
            &Message::text(Role::Assistant, "done"),
            Some(TokenUsage {
                input_tokens: Some(19),
                output_tokens: Some(2),
                total_tokens: Some(21),
                cached_input_tokens: None,
                reasoning_tokens: None,
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
    let summary: (String, i32, i32, Option<i64>, Option<i64>, Option<i64>) = sqlx::query_as(
        "SELECT status, model_call_count, tool_call_count,
                input_tokens, output_tokens, cached_input_tokens
         FROM turns WHERE id = $1",
    )
    .bind(turn_id.as_str())
    .fetch_one(storage.pool())
    .await
    .unwrap();
    assert_eq!(
        summary,
        ("completed".to_string(), 2, 1, Some(30), Some(9), Some(3))
    );

    let trace = PostgresTraceRecorder::spawn(storage.pool().clone());
    let model_span = ModelCallStarted {
        span_id: unique("span-model"),
        turn_id: turn_id.clone(),
        sequence: 1,
        model_id: Some(model_id.clone()),
        resolved_model_name: "deepseek-v4-flash".to_string(),
        started_at: OffsetDateTime::now_utc(),
    };
    trace.record(TraceSignal::ModelCallStarted(model_span.clone()));
    trace.record(TraceSignal::ModelCallFinished(ModelCallFinished {
        started: model_span.clone(),
        status: TraceStatus::Succeeded,
        provider_request_id: Some("provider-request-test".to_string()),
        attempt_count: 1,
        usage: None,
        ended_at: OffsetDateTime::now_utc(),
        error_code: None,
        error_message: None,
    }));
    let tool_span = ToolCallStarted {
        span_id: unique("span-tool"),
        turn_id: turn_id.clone(),
        parent_span_id: model_span.span_id.clone(),
        sequence: 2,
        provider_call_id: "provider-call-1".to_string(),
        requested_tool_name: "read_file".to_string(),
        started_at: OffsetDateTime::now_utc(),
    };
    trace.record(TraceSignal::ToolCallStarted(tool_span.clone()));
    trace.record(TraceSignal::ToolCallFinished(ToolCallFinished {
        started: tool_span,
        status: TraceStatus::Succeeded,
        resolved_tool_name: Some("read_file".to_string()),
        permission_wait_ms: Some(4),
        ended_at: OffsetDateTime::now_utc(),
        error_code: None,
        error_message: None,
    }));
    let flush = trace.flush_turn(&turn_id).await;
    assert!(flush.flushed);
    assert_eq!(flush.write_failures, 0);
    let trace_rows: Vec<(String, String, Option<String>)> = sqlx::query_as(
        "SELECT kind, status, parent_span_id
         FROM trace_spans WHERE turn_id = $1 ORDER BY sequence",
    )
    .bind(turn_id.as_str())
    .fetch_all(storage.pool())
    .await
    .unwrap();
    assert_eq!(
        trace_rows,
        vec![
            ("model_call".to_string(), "succeeded".to_string(), None),
            (
                "tool_call".to_string(),
                "succeeded".to_string(),
                Some(model_span.span_id)
            )
        ]
    );

    let interrupted_turn_id = TurnId::new(unique("turn-interrupted"));
    storage
        .begin_turn(
            &session_id,
            &interrupted_turn_id,
            &ClientRequestId::new(unique("request-interrupted")),
            &ResolvedModel::new(Some(model_id.clone()), "deepseek", "deepseek-v4-flash"),
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

    sqlx::query("DELETE FROM sessions WHERE id = $1")
        .bind(session_id.as_str())
        .execute(storage.pool())
        .await
        .unwrap();
    sqlx::query("DELETE FROM models WHERE id = $1")
        .bind(&model_id)
        .execute(storage.pool())
        .await
        .unwrap();
}
