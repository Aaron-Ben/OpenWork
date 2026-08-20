use std::{collections::HashSet, str::FromStr, sync::Arc};

use axum::{
    Json, Router,
    body::Body,
    extract::State,
    http::{HeaderMap, Response, header::CONTENT_TYPE},
    routing::post,
};
use openwork_collab::{
    model::{AgendaCandidate, AgendaCard, AgentInput, TriageSettings},
    storage::CollabStorage,
    triage::{
        AgendaTriageContext, DmLoopContext, TriageClient, TriageContext, TriageMessage, TriageRoom,
    },
};
use openwork_credentials::{ApiKeyCipher, PostgresCredentialStore, ProviderCredentialInput};
use serde_json::{Value, json};
use sqlx::{
    Executor, PgPool,
    postgres::{PgConnectOptions, PgPoolOptions},
};
use tokio::sync::Mutex;
use uuid::Uuid;

#[derive(Clone, Default)]
struct Captured(Arc<Mutex<Option<(String, Value)>>>);

async fn completion(
    State(captured): State<Captured>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Response<Body> {
    let authorization = headers
        .get("authorization")
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .to_string();
    *captured.0.lock().await = Some((authorization, body));
    let event = json!({
        "id": "response-1",
        "model": "cheap-model",
        "choices": [{
            "delta": {"content": "{\"actionable\":false,\"responseMode\":\"one-of-us\",\"reason\":\"unrelated\",\"promptNote\":\"stay quiet\"}"},
            "finish_reason": "stop"
        }],
        "usage": {"prompt_tokens": 17, "completion_tokens": 9, "total_tokens": 26}
    });
    Response::builder()
        .status(200)
        .header(CONTENT_TYPE, "text/event-stream")
        .body(Body::from(format!("data: {event}\n\ndata: [DONE]\n\n")))
        .unwrap()
}

#[tokio::test]
async fn triage_uses_the_configured_openwork_provider_api_and_records_usage_shape() {
    let Ok(database_url) = std::env::var("TEST_DATABASE_URL") else {
        return;
    };
    let admin = PgPool::connect(&database_url).await.unwrap();
    let schema = format!("collab_triage_test_{}", Uuid::new_v4().simple());
    admin
        .execute(format!("CREATE SCHEMA {schema}").as_str())
        .await
        .unwrap();
    let options = PgConnectOptions::from_str(&database_url)
        .unwrap()
        .options([("search_path", schema.as_str())]);
    let pool = PgPoolOptions::new()
        .max_connections(4)
        .connect_with(options)
        .await
        .unwrap();
    pool.execute(
        "CREATE TABLE provider_credentials (
            provider_id TEXT PRIMARY KEY, display_name TEXT NOT NULL,
            provider_kind TEXT NOT NULL, base_url TEXT NOT NULL,
            api_key_encrypted TEXT NOT NULL, enabled BOOLEAN NOT NULL,
            config JSONB NOT NULL, created_at TIMESTAMP WITHOUT TIME ZONE NOT NULL
                DEFAULT (CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai'),
            updated_at TIMESTAMP WITHOUT TIME ZONE NOT NULL
                DEFAULT (CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai')
        )",
    )
    .await
    .unwrap();
    let storage = CollabStorage::from_pool(pool.clone());
    storage.migrate().await.unwrap();
    let agent = storage
        .create_agent(&AgentInput {
            id: Some("alice".to_string()),
            display_name: "Alice".to_string(),
            role: Some("database specialist".to_string()),
            bio: None,
            system_prompt: "Help when relevant".to_string(),
            provider_id: "opencode".to_string(),
            model_id: "main".to_string(),
            enabled: true,
            scanner_enabled: false,
        })
        .await
        .unwrap();

    let captured = Captured::default();
    let app = Router::new()
        .route("/chat/completions", post(completion))
        .with_state(captured.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

    let credential_store =
        PostgresCredentialStore::new(pool.clone(), ApiKeyCipher::from_key([7; 32]));
    let mut transaction = pool.begin().await.unwrap();
    credential_store
        .insert(
            &mut transaction,
            ProviderCredentialInput {
                provider_id: "cheap",
                display_name: "Cheap support",
                provider_kind: "deepseek",
                base_url: &format!("http://{address}"),
                api_key: Some("support-secret"),
                enabled: true,
                config: &json!({}),
            },
        )
        .await
        .unwrap();
    transaction.commit().await.unwrap();

    let client = TriageClient::with_credential_store(credential_store);
    let messages = vec![TriageMessage {
        author_id: "user".to_string(),
        sequence: 1,
        body: "Question for someone else".to_string(),
    }];
    let settings = TriageSettings {
        provider_id: "cheap".to_string(),
        model_id: "cheap-model".to_string(),
    };
    let result = client
        .decide(
            &settings,
            TriageContext {
                agent: &agent,
                rooms: &[TriageRoom {
                    room_id: "general".to_string(),
                    kind: "group".to_string(),
                    messages: messages.clone(),
                    active_teammates: Some(HashSet::from(["bob".to_string()])),
                }],
            },
        )
        .await
        .unwrap();
    assert!(!result.decision.actionable);
    assert_eq!(result.input_tokens, Some(17));
    assert_eq!(result.output_tokens, Some(9));
    let (authorization, request) = captured.0.lock().await.clone().unwrap();
    assert_eq!(authorization, "Bearer support-secret");
    assert_eq!(request["model"], "cheap-model");
    assert!(request.to_string().contains("activeTeammates"));
    assert!(request.to_string().contains("\\\"bob\\\":true"));
    assert!(
        request
            .to_string()
            .contains("an explicitly requested reaction is actionable"),
        "triage must wake an Agent for react even when no prose reply is needed"
    );
    let agenda = AgendaCandidate {
        room_id: "general".to_string(),
        highest_sequence: 1,
        cards: vec![AgendaCard {
            id: "card_1".to_string(),
            title: "Do shared work".to_string(),
            description: None,
            assignee_id: Some("alice".to_string()),
        }],
        stalled: false,
        recent_messages: Vec::new(),
    };
    let agenda_result = client
        .decide_agenda(
            &settings,
            AgendaTriageContext {
                agent: &agent,
                candidate: &agenda,
            },
        )
        .await
        .unwrap();
    assert!(!agenda_result.decision.actionable);
    let (_, agenda_request) = captured.0.lock().await.clone().unwrap();
    assert!(
        agenda_request
            .to_string()
            .contains("Before spending an OpenCode main-reasoning turn")
    );
    assert_eq!(agenda_request["model"], "cheap-model");
    let dm_result = client
        .decide_dm_progress(
            &settings,
            DmLoopContext {
                agent: &agent,
                room_id: "dm_alice_bob",
                messages: &messages,
            },
        )
        .await
        .unwrap();
    assert!(!dm_result.decision.actionable);
    let (_, dm_request) = captured.0.lock().await.clone().unwrap();
    assert!(
        dm_request
            .to_string()
            .contains("mandatory every-eighth-message check")
    );

    server.abort();
    pool.close().await;
    admin
        .execute(format!("DROP SCHEMA {schema} CASCADE").as_str())
        .await
        .unwrap();
}
