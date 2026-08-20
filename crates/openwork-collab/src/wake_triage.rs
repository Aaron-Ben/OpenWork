//! Cheap-model gates for message-driven wakes, including Agent-DM loop checks.

use std::collections::{HashMap, HashSet};

use crate::{
    mcp::MessageNotice,
    model::TriageRecordInput,
    observation::{ObservationSink, record_triage},
    proactivity::{DmProgress, resolve_dm_progress, should_probe_agent_dm},
    storage::CollabStorage,
    triage::{
        DmLoopContext, TriageClient, TriageContext, TriageMessage, TriageRoom, TriageSource,
        resolve_failure,
    },
};

pub struct WakeTriage {
    pub actionable: bool,
    pub prompt_note: Option<String>,
}

/// One cheap-model decision for an Agent's whole pending inbox.
///
/// Judging room by room would spend a triage call per room and still hand the
/// engine the cross-room inbox, because an Agent has exactly one session. So
/// the gate matches the delivery: one decision, one wake, and per-room records
/// so `collab_triages` still says what was decided about each room.
///
/// Agent-to-Agent DM rooms keep their own gate: the loop probe is a different
/// question ("is this exchange still going anywhere?") with its own source, and
/// folding it into the general decision would lose that.
pub async fn evaluate(
    storage: &CollabStorage,
    triage: &TriageClient,
    agent_id: &str,
    batches: &HashMap<String, Vec<MessageNotice>>,
    active_teammates: &HashMap<String, HashSet<String>>,
    observations: &ObservationSink,
) -> WakeTriage {
    if batches.is_empty() {
        return WakeTriage {
            actionable: false,
            prompt_note: None,
        };
    }
    // A human anywhere in the inbox makes the whole decision fail open, and
    // hides the "who else is awake" signal everywhere. Silence toward a waiting
    // person is the worst outcome this gate can produce.
    let human_waiting = batches
        .values()
        .flatten()
        .any(|notice| notice.author_id == "user");
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

    let mut rooms = Vec::new();
    let mut dm_actionable = false;
    let mut dm_prompt_note = None;
    for (room_id, batch) in batches {
        let Some(up_to_sequence) = batch.iter().map(|notice| notice.sequence).max() else {
            continue;
        };
        let direct_exchange = match storage.agent_direct_exchange(room_id, 32).await {
            Ok(exchange) => exchange,
            Err(error) => {
                eprintln!("failed to inspect direct room {room_id}: {error}");
                None
            }
        };
        if let Some(exchange) = direct_exchange {
            let must_probe = batch
                .iter()
                .any(|notice| should_probe_agent_dm(notice.sequence));
            let verdict = evaluate_agent_dm(
                storage,
                triage,
                AgentDmEvaluation {
                    settings: settings.as_ref(),
                    agent: &agent,
                    room_id,
                    up_to_sequence,
                    must_probe,
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
            if verdict.actionable {
                dm_actionable = true;
                dm_prompt_note = dm_prompt_note.or(verdict.prompt_note);
            }
            continue;
        }
        rooms.push(TriageRoom {
            room_id: room_id.clone(),
            kind: "group".to_string(),
            messages: batch
                .iter()
                .map(|notice| TriageMessage {
                    author_id: notice.author_id.clone(),
                    sequence: notice.sequence,
                    body: notice.body.clone(),
                })
                .collect(),
            active_teammates: triage_active_teammates(human_waiting, active_teammates.get(room_id)),
        });
    }

    if rooms.is_empty() {
        return WakeTriage {
            actionable: dm_actionable,
            prompt_note: dm_prompt_note,
        };
    }

    let started = std::time::Instant::now();
    let (actionable, response_mode, source, reason, prompt_note, input_tokens, output_tokens) =
        match settings.as_ref() {
            Some(settings) => match triage
                .decide(
                    settings,
                    TriageContext {
                        agent: &agent,
                        rooms: &rooms,
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
    // One decision, but a record per room: collab_triages stays keyed by
    // (agent, room, up_to_seq) so the log drawer can still answer "what was
    // decided about this room, up to which message".
    for room in &rooms {
        let up_to_sequence = room
            .messages
            .iter()
            .map(|message| message.sequence)
            .max()
            .unwrap_or_default();
        record(
            storage,
            agent_id,
            &room.room_id,
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
    }
    WakeTriage {
        actionable: actionable || dm_actionable,
        prompt_note: prompt_note.or(dm_prompt_note),
    }
}

/// The "who else is awake" signal, or nothing at all while a human waits.
///
/// Returning `None` must be indistinguishable from having no busy teammates:
/// the triage prompt then carries no `activeTeammates` key and reads exactly as
/// it did before the signal existed.
fn triage_active_teammates(
    human_waiting: bool,
    active_teammates: Option<&HashSet<String>>,
) -> Option<HashSet<String>> {
    if human_waiting {
        return None;
    }
    active_teammates.filter(|set| !set.is_empty()).cloned()
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
    use std::{
        collections::{HashMap, HashSet},
        str::FromStr,
        sync::Arc,
    };

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

    #[test]
    fn active_teammates_are_completely_hidden_while_a_human_waits() {
        let active = HashSet::from(["alice".to_string(), "bob".to_string()]);
        assert_eq!(super::triage_active_teammates(true, Some(&active)), None);
        assert_eq!(
            super::triage_active_teammates(false, Some(&active)),
            Some(active.clone())
        );
        assert_eq!(
            super::triage_active_teammates(false, Some(&HashSet::new())),
            None
        );
        assert_eq!(super::triage_active_teammates(false, None), None);
    }

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

    async fn actionable(Json(_body): Json<Value>) -> Response<Body> {
        let event = json!({
            "id": "cross-room-response",
            "model": "cheap-model",
            "choices": [{
                "delta": {"content": "{\"actionable\":true,\"responseMode\":\"one-of-us\",\"reason\":\"the inbox needs attention\",\"promptNote\":\"engage\"}"},
                "finish_reason": "stop"
            }],
            "usage": {"prompt_tokens": 23, "completion_tokens": 7, "total_tokens": 30}
        });
        Response::builder()
            .status(200)
            .header(CONTENT_TYPE, "text/event-stream")
            .body(Body::from(format!("data: {event}\n\ndata: [DONE]\n\n")))
            .unwrap()
    }

    #[tokio::test]
    async fn one_cross_room_decision_records_each_rooms_own_highest_sequence() {
        let Ok(database_url) = std::env::var("TEST_DATABASE_URL") else {
            return;
        };
        let admin = PgPool::connect(&database_url).await.unwrap();
        let schema = format!("collab_cross_room_triage_test_{}", Uuid::new_v4().simple());
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
        storage
            .create_agent(&AgentInput {
                id: Some("alice".to_string()),
                display_name: "Alice".to_string(),
                role: None,
                bio: None,
                system_prompt: "Help when relevant".to_string(),
                provider_id: "opencode".to_string(),
                model_id: "main".to_string(),
                enabled: true,
                scanner_enabled: false,
            })
            .await
            .unwrap();
        for room_id in ["alpha", "beta"] {
            storage
                .create_group_room(Some(room_id), room_id)
                .await
                .unwrap();
        }

        let app = Router::new().route("/chat/completions", post(actionable));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let credential_store =
            PostgresCredentialStore::new(pool.clone(), ApiKeyCipher::from_key([8; 32]));
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
        let batches = HashMap::from([
            (
                "alpha".to_string(),
                vec![
                    MessageNotice {
                        room_id: "alpha".to_string(),
                        author_id: "user".to_string(),
                        body: "alpha earlier".to_string(),
                        sequence: 3,
                    },
                    MessageNotice {
                        room_id: "alpha".to_string(),
                        author_id: "user".to_string(),
                        body: "alpha latest".to_string(),
                        sequence: 8,
                    },
                ],
            ),
            (
                "beta".to_string(),
                vec![MessageNotice {
                    room_id: "beta".to_string(),
                    author_id: "user".to_string(),
                    body: "beta latest".to_string(),
                    sequence: 5,
                }],
            ),
        ]);

        let verdict = evaluate(
            &storage,
            &triage,
            "alice",
            &batches,
            &HashMap::new(),
            &observations,
        )
        .await;

        assert!(verdict.actionable);
        let records = storage.triage_records(None).await.unwrap();
        assert_eq!(records.len(), 2);
        let by_room = records
            .into_iter()
            .map(|record| (record.room_id.clone(), record))
            .collect::<HashMap<_, _>>();
        assert_eq!(by_room["alpha"].up_to_sequence, 8);
        assert_eq!(by_room["beta"].up_to_sequence, 5);
        assert!(
            by_room
                .values()
                .all(|record| record.source == "support_model")
        );

        server.abort();
        pool.close().await;
        admin
            .execute(format!("DROP SCHEMA {schema} CASCADE").as_str())
            .await
            .unwrap();
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
                    id: Some(id.to_string()),
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
            &HashMap::from([(
                room.id.clone(),
                vec![MessageNotice {
                    room_id: room.id.clone(),
                    author_id: "alice".to_string(),
                    body: "loop step 7".to_string(),
                    sequence: 7,
                }],
            )]),
            &HashMap::new(),
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
            &HashMap::from([(
                room.id.clone(),
                vec![MessageNotice {
                    room_id: room.id.clone(),
                    author_id: "alice".to_string(),
                    body: "loop step 8".to_string(),
                    sequence: 8,
                }],
            )]),
            &HashMap::new(),
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
