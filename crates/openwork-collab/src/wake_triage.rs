//! Cheap-model gates for message-driven wakes, including Agent-DM loop checks.

use crate::{
    mcp::MessageNotice,
    model::TriageRecordInput,
    observation::{ObservationSink, record_triage},
    proactivity::{DmProgress, resolve_dm_progress, should_probe_agent_dm},
    storage::CollabStorage,
    triage::{
        DmLoopContext, TriageClient, TriageContext, TriageMessage, TriageSource, resolve_failure,
    },
};

pub struct WakeTriage {
    pub actionable: bool,
    pub prompt_note: Option<String>,
}

pub async fn evaluate(
    storage: &CollabStorage,
    triage: &TriageClient,
    agent_id: &str,
    room_id: &str,
    batch: &[MessageNotice],
    observations: &ObservationSink,
) -> WakeTriage {
    let Some(up_to_sequence) = batch.iter().map(|notice| notice.sequence).max() else {
        return WakeTriage {
            actionable: false,
            prompt_note: None,
        };
    };
    let human_waiting = batch.iter().any(|notice| notice.author_id == "user");
    let messages = batch
        .iter()
        .map(|notice| TriageMessage {
            author_id: notice.author_id.clone(),
            sequence: notice.sequence,
            body: notice.body.clone(),
        })
        .collect::<Vec<_>>();
    let agent = match storage.agent(agent_id).await {
        Ok(Some(agent)) => agent,
        Ok(None) => {
            return WakeTriage {
                actionable: false,
                prompt_note: None,
            };
        }
        Err(error) => {
            eprintln!("failed to load Agent {agent_id} for triage: {error}");
            return WakeTriage {
                actionable: human_waiting,
                prompt_note: None,
            };
        }
    };
    let settings = match storage.triage_settings().await {
        Ok(settings) => settings,
        Err(error) => {
            eprintln!("failed to load triage settings: {error}");
            None
        }
    };
    let must_probe_dm = batch
        .iter()
        .any(|notice| should_probe_agent_dm(notice.sequence));

    match storage.agent_direct_exchange(room_id, 32).await {
        Ok(Some(exchange)) => {
            return evaluate_agent_dm(
                storage,
                triage,
                AgentDmEvaluation {
                    settings: settings.as_ref(),
                    agent: &agent,
                    room_id,
                    up_to_sequence,
                    must_probe: must_probe_dm,
                    messages: &exchange
                        .into_iter()
                        .map(|message| TriageMessage {
                            author_id: message.author_id,
                            sequence: message.sequence,
                            body: message.body,
                        })
                        .collect::<Vec<_>>(),
                    observations,
                },
            )
            .await;
        }
        Ok(None) => {}
        Err(error) => eprintln!("failed to inspect direct room {room_id}: {error}"),
    }

    let started = std::time::Instant::now();
    let (actionable, response_mode, source, reason, prompt_note, input_tokens, output_tokens) =
        match settings.as_ref() {
            Some(settings) => match triage
                .decide(
                    settings,
                    TriageContext {
                        agent: &agent,
                        room_id,
                        messages: &messages,
                    },
                )
                .await
            {
                Ok(result) => (
                    result.decision.actionable,
                    Some(result.decision.response_mode.as_database_str()),
                    TriageSource::SupportModel,
                    Some(result.decision.reason),
                    Some(result.decision.prompt_note),
                    result.input_tokens,
                    result.output_tokens,
                ),
                Err(error) => {
                    let fallback = resolve_failure(human_waiting, error.to_string());
                    (
                        fallback.actionable,
                        None,
                        fallback.source,
                        Some(fallback.reason),
                        None,
                        None,
                        None,
                    )
                }
            },
            None => {
                let fallback = resolve_failure(human_waiting, "triage model is not configured");
                (
                    fallback.actionable,
                    None,
                    fallback.source,
                    Some(fallback.reason),
                    None,
                    None,
                    None,
                )
            }
        };
    record(
        storage,
        agent_id,
        room_id,
        up_to_sequence,
        actionable,
        response_mode,
        source,
        reason.as_deref(),
        prompt_note.as_deref(),
        settings.as_ref().map(|value| value.provider_id.as_str()),
        settings.as_ref().map(|value| value.model_id.as_str()),
        input_tokens,
        output_tokens,
        started,
        observations,
    )
    .await;
    WakeTriage {
        actionable,
        prompt_note,
    }
}

struct AgentDmEvaluation<'a> {
    settings: Option<&'a crate::model::TriageSettings>,
    agent: &'a crate::model::Agent,
    room_id: &'a str,
    up_to_sequence: i64,
    must_probe: bool,
    messages: &'a [TriageMessage],
    observations: &'a ObservationSink,
}

async fn evaluate_agent_dm(
    storage: &CollabStorage,
    triage: &TriageClient,
    evaluation: AgentDmEvaluation<'_>,
) -> WakeTriage {
    let started = std::time::Instant::now();
    let (actionable, source, reason, prompt_note, response_mode, input_tokens, output_tokens) =
        if !evaluation.must_probe {
            (
                true,
                TriageSource::DmAgentEngage,
                "Agent direct exchange continues until the next eight-message progress check"
                    .to_string(),
                Some("Continue only if you can add concrete progress.".to_string()),
                None,
                None,
                None,
            )
        } else {
            match evaluation.settings {
                Some(settings) => match triage
                    .decide_dm_progress(
                        settings,
                        DmLoopContext {
                            agent: evaluation.agent,
                            room_id: evaluation.room_id,
                            messages: evaluation.messages,
                        },
                    )
                    .await
                {
                    Ok(result) => {
                        let actionable = result.decision.actionable;
                        (
                            actionable,
                            match resolve_dm_progress(actionable) {
                                DmProgress::Continue => TriageSource::DmAgentEngage,
                                DmProgress::Stop => TriageSource::LoopCap,
                            },
                            result.decision.reason,
                            actionable.then_some(result.decision.prompt_note),
                            Some(result.decision.response_mode.as_database_str()),
                            result.input_tokens,
                            result.output_tokens,
                        )
                    }
                    Err(error) => {
                        let fallback = resolve_failure(false, error.to_string());
                        (
                            false,
                            fallback.source,
                            fallback.reason,
                            None,
                            None,
                            None,
                            None,
                        )
                    }
                },
                None => {
                    let fallback = resolve_failure(false, "DM loop detector is not configured");
                    (
                        false,
                        fallback.source,
                        fallback.reason,
                        None,
                        None,
                        None,
                        None,
                    )
                }
            }
        };
    record(
        storage,
        &evaluation.agent.id,
        evaluation.room_id,
        evaluation.up_to_sequence,
        actionable,
        response_mode,
        source,
        Some(&reason),
        prompt_note.as_deref(),
        evaluation.settings.map(|value| value.provider_id.as_str()),
        evaluation.settings.map(|value| value.model_id.as_str()),
        input_tokens,
        output_tokens,
        started,
        evaluation.observations,
    )
    .await;
    WakeTriage {
        actionable,
        prompt_note,
    }
}

#[allow(clippy::too_many_arguments)]
async fn record(
    storage: &CollabStorage,
    agent_id: &str,
    room_id: &str,
    up_to_sequence: i64,
    actionable: bool,
    response_mode: Option<&str>,
    source: TriageSource,
    reason: Option<&str>,
    prompt_note: Option<&str>,
    provider_id: Option<&str>,
    model_id: Option<&str>,
    input_tokens: Option<i64>,
    output_tokens: Option<i64>,
    started: std::time::Instant,
    observations: &ObservationSink,
) {
    let latency_ms = i64::try_from(started.elapsed().as_millis()).unwrap_or(i64::MAX);
    if let Err(error) = record_triage(
        storage,
        observations,
        TriageRecordInput {
            agent_id,
            room_id,
            up_to_sequence,
            actionable,
            response_mode,
            source: source.as_str(),
            reason,
            prompt_note,
            provider_id,
            model_id,
            input_tokens,
            output_tokens,
            latency_ms,
        },
    )
    .await
    {
        eprintln!("failed to record triage for Agent {agent_id}: {error}");
    }
}

#[cfg(test)]
mod tests {
    use std::{str::FromStr, sync::Arc};

    use axum::{
        Json, Router,
        body::Body,
        extract::State,
        http::{Response, header::CONTENT_TYPE},
        routing::post,
    };
    use openwork_credentials::{ApiKeyCipher, PostgresCredentialStore, ProviderCredentialInput};
    use serde_json::{Value, json};
    use sqlx::{
        Executor, PgPool,
        postgres::{PgConnectOptions, PgPoolOptions},
    };
    use tokio::sync::Mutex;
    use uuid::Uuid;

    use super::evaluate;
    use crate::{
        mcp::MessageNotice, model::AgentInput, storage::CollabStorage, triage::TriageClient,
    };

    #[derive(Clone, Default)]
    struct Calls(Arc<Mutex<u32>>);

    async fn no_progress(State(calls): State<Calls>, Json(_body): Json<Value>) -> Response<Body> {
        *calls.0.lock().await += 1;
        let event = json!({
            "id": "dm-loop-response",
            "model": "cheap-model",
            "choices": [{
                "delta": {"content": "{\"actionable\":false,\"responseMode\":\"one-of-us\",\"reason\":\"the exchange is repeating\",\"promptNote\":\"\"}"},
                "finish_reason": "stop"
            }],
            "usage": {"prompt_tokens": 31, "completion_tokens": 8, "total_tokens": 39}
        });
        Response::builder()
            .status(200)
            .header(CONTENT_TYPE, "text/event-stream")
            .body(Body::from(format!("data: {event}\n\ndata: [DONE]\n\n")))
            .unwrap()
    }

    #[tokio::test]
    async fn agent_dm_defaults_to_engage_then_stops_at_the_eighth_no_progress_message() {
        let Ok(database_url) = std::env::var("TEST_DATABASE_URL") else {
            return;
        };
        let admin = PgPool::connect(&database_url).await.unwrap();
        let schema = format!("collab_dm_loop_test_{}", Uuid::new_v4().simple());
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
        for id in ["alice", "bob"] {
            storage
                .create_agent(&AgentInput {
                    id: id.to_string(),
                    display_name: id.to_string(),
                    role: None,
                    bio: None,
                    system_prompt: "Advance the direct exchange".to_string(),
                    provider_id: "opencode".to_string(),
                    model_id: "main".to_string(),
                    enabled: true,
                    scanner_enabled: false,
                })
                .await
                .unwrap();
        }
        let room = storage.create_direct_room("alice", "bob").await.unwrap();
        for sequence in 1..=7 {
            let author = if sequence % 2 == 0 { "bob" } else { "alice" };
            storage
                .send_message(&room.id, author, &format!("loop step {sequence}"))
                .await
                .unwrap();
        }

        let calls = Calls::default();
        let app = Router::new()
            .route("/chat/completions", post(no_progress))
            .with_state(calls.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let credential_store =
            PostgresCredentialStore::new(pool.clone(), ApiKeyCipher::from_key([9; 32]));
        let mut transaction = pool.begin().await.unwrap();
        credential_store
            .insert(
                &mut transaction,
                ProviderCredentialInput {
                    provider_id: "cheap",
                    display_name: "Cheap support",
                    provider_kind: "deepseek",
                    base_url: &format!("http://{address}"),
                    api_key: Some("test-secret"),
                    enabled: true,
                    config: &json!({}),
                },
            )
            .await
            .unwrap();
        transaction.commit().await.unwrap();
        sqlx::query(
            "UPDATE collab_settings
                SET triage_provider_id = 'cheap', triage_model_id = 'cheap-model'
              WHERE id = 'singleton'",
        )
        .execute(&pool)
        .await
        .unwrap();
        let triage = TriageClient::with_credential_store(credential_store);
        let observations = crate::observation::ObservationSink::discarding();

        let seventh = evaluate(
            &storage,
            &triage,
            "bob",
            &room.id,
            &[MessageNotice {
                room_id: room.id.clone(),
                author_id: "alice".to_string(),
                body: "loop step 7".to_string(),
                sequence: 7,
            }],
            &observations,
        )
        .await;
        assert!(seventh.actionable);
        assert_eq!(
            *calls.0.lock().await,
            0,
            "messages 1-7 spend no loop-check call"
        );

        storage
            .send_message(&room.id, "alice", "loop step 8")
            .await
            .unwrap();
        let eighth = evaluate(
            &storage,
            &triage,
            "bob",
            &room.id,
            &[MessageNotice {
                room_id: room.id.clone(),
                author_id: "alice".to_string(),
                body: "loop step 8".to_string(),
                sequence: 8,
            }],
            &observations,
        )
        .await;
        assert!(!eighth.actionable);
        assert_eq!(*calls.0.lock().await, 1);
        let records = storage.triage_records(Some(&room.id)).await.unwrap();
        assert_eq!(records[0].source, "loop_cap");
        assert!(!records[0].actionable);
        assert_eq!(records[0].input_tokens, Some(31));

        server.abort();
        pool.close().await;
        admin
            .execute(format!("DROP SCHEMA {schema} CASCADE").as_str())
            .await
            .unwrap();
    }
}
