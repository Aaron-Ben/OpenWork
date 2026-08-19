use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{Arc, RwLock},
};

use openwork_credentials::PostgresCredentialStore;
use openwork_models::provider::ProviderKind;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use thiserror::Error;
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::{UnixListener, UnixStream},
    sync::{broadcast, mpsc},
};
use tokio_util::sync::CancellationToken;

use crate::{
    activity::AgentRuntimeRegistry,
    card_events, claim_reaper,
    coordination::CoordinationHub,
    event::{CollabEventKind, CollabEventPublisher},
    home::{HomeError, HomeManager},
    mcp::{IdentityRegistry, MessageNotice, start_server},
    model::{AgentInput, AgentView, CardInput, MessagePageAnchor, MessagePageQuery},
    observation,
    opencode::{OpenCodeError, OpenCodeSupervisor, PermissionReply},
    permission::PermissionTracker,
    scheduler::{AgentTokens, start as start_scheduler},
    storage::{CollabStorage, StorageError},
};

pub use crate::daemon_config::{COLLAB_HOME_ENV, DaemonConfig};

pub async fn run(config: DaemonConfig) -> Result<(), DaemonError> {
    tokio::fs::create_dir_all(&config.root).await?;
    let socket = bind_single_instance(&config.socket_path()).await?;
    println!("daemon.socket={}", socket.path.display());

    let storage = CollabStorage::connect(config.database_url.as_deref()).await?;
    storage.migrate().await?;
    let repaired_runs = storage.repair_interrupted_runs().await?;
    println!("daemon.startup.repaired_runs={repaired_runs}");
    let events = CollabEventPublisher::default();
    let released_claims = storage.release_all_claims().await?;
    println!("daemon.startup.released_claims={}", released_claims.count);
    for room_id in released_claims.room_ids {
        events
            .publish(CollabEventKind::BoardsChanged { room_id })
            .await;
    }

    let cancel = CancellationToken::new();
    let (observations, observation_worker) =
        observation::start(storage.clone(), events.clone(), cancel.clone());
    let gc_worker = crate::gc::start(storage.clone(), config.gc_policy, cancel.clone());
    let identities = IdentityRegistry::default();
    let tokens: AgentTokens = Arc::new(RwLock::new(HashMap::new()));
    let (notice_tx, notice_rx) = mpsc::unbounded_channel();
    let coordination = CoordinationHub::default();
    let runtime = AgentRuntimeRegistry::default();
    let mcp = start_server(
        storage.clone(),
        identities.clone(),
        notice_tx.clone(),
        coordination.clone(),
        events.clone(),
        observations.clone(),
        cancel.clone(),
    )
    .await?;
    println!("daemon.mcp={}", mcp.url);
    let homes = HomeManager::new(&config.root, &mcp.url);
    for agent in storage.agents().await? {
        let token = identities.issue(&agent.id).map_err(DaemonError::Identity)?;
        tokens
            .write()
            .map_err(|_| DaemonError::Identity("Agent token lock is poisoned".to_string()))?
            .insert(agent.id.clone(), token.clone());
        homes.repair(&agent, &token).await?;
    }

    let supervisor = OpenCodeSupervisor::start(cancel.clone()).await?;
    let initial = supervisor.current()?;
    println!(
        "daemon.opencode.version={} pid={} generation={}",
        initial.version,
        initial
            .pid
            .map_or_else(|| "unknown".to_string(), |pid| pid.to_string()),
        initial.generation
    );
    let permissions = PermissionTracker::default();
    let scheduler = start_scheduler(
        storage.clone(),
        homes.clone(),
        tokens.clone(),
        supervisor.subscribe(),
        notice_rx,
        permissions.clone(),
        events.clone(),
        coordination,
        runtime.clone(),
        observations,
        cancel.clone(),
    )
    .await?;
    let claim_reaper = claim_reaper::start(
        storage.clone(),
        homes.clone(),
        supervisor.subscribe(),
        notice_tx.clone(),
        events.clone(),
        cancel.clone(),
    );
    let context = DaemonContext {
        storage,
        homes,
        identities,
        tokens,
        notices: notice_tx,
        permissions,
        events,
        runtime,
        engine: supervisor.subscribe(),
        cancel: cancel.clone(),
    };

    println!("daemon.ready=true");
    loop {
        tokio::select! {
            _ = cancel.cancelled() => break,
            signal = tokio::signal::ctrl_c() => {
                signal?;
                cancel.cancel();
            }
            accepted = socket.listener.accept() => {
                let (stream, _) = accepted?;
                let context = context.clone();
                tokio::spawn(async move {
                    if let Err(error) = serve_connection(stream, context).await {
                        eprintln!("daemon IPC request failed: {error}");
                    }
                });
            }
        }
    }

    cancel.cancel();
    scheduler.shutdown().await;
    claim_reaper.shutdown().await;
    gc_worker.shutdown().await;
    observation_worker.shutdown().await;
    supervisor.shutdown().await;
    mcp.shutdown().await;
    drop(socket);
    println!("daemon.stopped=true");
    Ok(())
}

#[derive(Clone)]
struct DaemonContext {
    storage: CollabStorage,
    homes: HomeManager,
    identities: IdentityRegistry,
    tokens: AgentTokens,
    notices: mpsc::UnboundedSender<MessageNotice>,
    permissions: PermissionTracker,
    events: CollabEventPublisher,
    runtime: AgentRuntimeRegistry,
    engine: tokio::sync::watch::Receiver<Option<crate::opencode::EngineConnection>>,
    cancel: CancellationToken,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum IpcRequest {
    Ping,
    Status,
    Shutdown,
    CreateAgent {
        agent: AgentInput,
    },
    UpdateAgent {
        agent: AgentInput,
    },
    ListAgents,
    CreateRoom {
        id: Option<String>,
        title: String,
    },
    CreateDirectRoom {
        first_participant: String,
        second_participant: String,
    },
    AddMember {
        room_id: String,
        participant_id: String,
    },
    SendMessage {
        room_id: String,
        author_id: String,
        body: String,
    },
    Messages {
        room_id: String,
    },
    ListRooms,
    MessagePage {
        room_id: String,
        anchor: Option<MessagePageAnchor>,
        limit: u32,
    },
    MarkRead {
        room_id: String,
        through_sequence: i64,
    },
    Permissions,
    ReplyPermission {
        id: String,
        reply: PermissionReply,
        message: Option<String>,
    },
    AbortPermission {
        id: String,
    },
    SubscribeEvents {
        after_sequence: u64,
    },
    CredentialCheck {
        provider_id: String,
    },
    ConfigureTriage {
        provider_id: String,
        model_id: String,
    },
    ListTriages {
        room_id: Option<String>,
    },
    ListLogs {
        room_id: Option<String>,
        limit: u32,
    },
    CreateBoard {
        id: String,
        room_id: String,
        title: String,
    },
    CreateBoardColumn {
        id: String,
        board_id: String,
        title: String,
        position: i32,
        is_done: bool,
    },
    ListBoards {
        room_id: String,
    },
    CreateCard {
        card: CardInput,
    },
    MoveCard {
        card_id: String,
        column_id: String,
        position: i32,
    },
    ReleaseCardClaim {
        card_id: String,
        claimed_by: String,
    },
}

#[derive(Debug, Serialize, Deserialize)]
pub struct IpcResponse {
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl IpcResponse {
    fn success(data: impl Serialize) -> Self {
        match serde_json::to_value(data) {
            Ok(data) => Self {
                ok: true,
                data: Some(data),
                error: None,
            },
            Err(error) => Self::failure(error.to_string()),
        }
    }

    fn failure(error: impl ToString) -> Self {
        Self {
            ok: false,
            data: None,
            error: Some(error.to_string()),
        }
    }
}

async fn serve_connection(stream: UnixStream, context: DaemonContext) -> Result<(), DaemonError> {
    let (read, mut write) = stream.into_split();
    let mut line = String::new();
    BufReader::new(read).read_line(&mut line).await?;
    if line.is_empty() {
        return Ok(());
    }
    let response = match serde_json::from_str::<IpcRequest>(&line) {
        Ok(IpcRequest::SubscribeEvents { after_sequence }) => {
            return serve_event_subscription(&mut write, &context, after_sequence).await;
        }
        Ok(request) => handle_request(request, &context).await,
        Err(error) => IpcResponse::failure(format!("invalid IPC request: {error}")),
    };
    let mut encoded = serde_json::to_vec(&response)?;
    encoded.push(b'\n');
    write.write_all(&encoded).await?;
    Ok(())
}

async fn serve_event_subscription(
    write: &mut tokio::net::unix::OwnedWriteHalf,
    context: &DaemonContext,
    after_sequence: u64,
) -> Result<(), DaemonError> {
    let mut subscription = context.events.subscribe(after_sequence).await;
    for event in subscription.replay {
        write_json_line(write, &event).await?;
    }
    loop {
        tokio::select! {
            _ = context.cancel.cancelled() => return Ok(()),
            event = subscription.receiver.recv() => match event {
                Ok(event) => write_json_line(write, &event).await?,
                Err(broadcast::error::RecvError::Lagged(_)) => continue,
                Err(broadcast::error::RecvError::Closed) => return Ok(()),
            }
        }
    }
}

async fn write_json_line(
    write: &mut tokio::net::unix::OwnedWriteHalf,
    value: &impl Serialize,
) -> Result<(), DaemonError> {
    let mut encoded = serde_json::to_vec(value)?;
    encoded.push(b'\n');
    write.write_all(&encoded).await?;
    Ok(())
}

async fn handle_request(request: IpcRequest, context: &DaemonContext) -> IpcResponse {
    match handle_request_inner(request, context).await {
        Ok(response) => response,
        Err(error) => IpcResponse::failure(error),
    }
}

async fn handle_request_inner(
    request: IpcRequest,
    context: &DaemonContext,
) -> Result<IpcResponse, DaemonError> {
    match request {
        IpcRequest::Ping => Ok(IpcResponse::success(json!({"pong": true}))),
        IpcRequest::Status => {
            let engine = context.engine.borrow().clone();
            Ok(IpcResponse::success(json!({
                "engine": engine.map(|engine| json!({
                    "generation": engine.generation,
                    "pid": engine.pid,
                    "version": engine.version.to_string(),
                    "baseUrl": engine.client.base_url(),
                }))
            })))
        }
        IpcRequest::Shutdown => {
            context.cancel.cancel();
            Ok(IpcResponse::success(json!({"shuttingDown": true})))
        }
        IpcRequest::CreateAgent { agent } => {
            let created = context.storage.create_agent(&agent).await?;
            let token = context
                .identities
                .issue(&created.id)
                .map_err(DaemonError::Identity)?;
            context
                .tokens
                .write()
                .map_err(|_| DaemonError::Identity("Agent token lock is poisoned".to_string()))?
                .insert(created.id.clone(), token.clone());
            context.homes.repair(&created, &token).await?;
            context.events.publish(CollabEventKind::AgentsChanged).await;
            Ok(IpcResponse::success(created))
        }
        IpcRequest::UpdateAgent { agent } => {
            let updated = context.storage.update_agent(&agent).await?;
            let token = context
                .tokens
                .read()
                .map_err(|_| DaemonError::Identity("Agent token lock is poisoned".to_string()))?
                .get(&updated.id)
                .cloned()
                .ok_or_else(|| {
                    DaemonError::Identity(format!("Agent {} has no token", updated.id))
                })?;
            context.homes.repair(&updated, &token).await?;
            context.events.publish(CollabEventKind::AgentsChanged).await;
            Ok(IpcResponse::success(updated))
        }
        IpcRequest::ListAgents => {
            let mut agents = Vec::new();
            for agent in context.storage.agents().await? {
                let activity = context.runtime.activity(&agent.id).await;
                agents.push(AgentView { agent, activity });
            }
            Ok(IpcResponse::success(agents))
        }
        IpcRequest::CreateRoom { id, title } => {
            let room = context
                .storage
                .create_group_room(id.as_deref(), &title)
                .await?;
            context.storage.add_member(&room.id, "user").await?;
            context
                .events
                .publish(CollabEventKind::RoomsChanged {
                    room_id: room.id.clone(),
                })
                .await;
            Ok(IpcResponse::success(room))
        }
        IpcRequest::CreateDirectRoom {
            first_participant,
            second_participant,
        } => {
            let room = context
                .storage
                .create_direct_room(&first_participant, &second_participant)
                .await?;
            context
                .events
                .publish(CollabEventKind::RoomsChanged {
                    room_id: room.id.clone(),
                })
                .await;
            Ok(IpcResponse::success(room))
        }
        IpcRequest::AddMember {
            room_id,
            participant_id,
        } => {
            context
                .storage
                .add_member(&room_id, &participant_id)
                .await?;
            context
                .events
                .publish(CollabEventKind::RoomsChanged { room_id })
                .await;
            Ok(IpcResponse::success(json!({"added": true})))
        }
        IpcRequest::SendMessage {
            room_id,
            author_id,
            body,
        } => {
            if !context.storage.is_member(&room_id, &author_id).await? {
                return Err(DaemonError::InvalidRequest(format!(
                    "participant {author_id} is not a member of room {room_id}"
                )));
            }
            let outcome = context
                .storage
                .send_message(&room_id, &author_id, &body)
                .await?;
            if !outcome.deduplicated {
                let _ = context.notices.send(MessageNotice {
                    room_id,
                    author_id,
                    body,
                    sequence: outcome.message.sequence,
                });
            }
            Ok(IpcResponse::success(outcome))
        }
        IpcRequest::Messages { room_id } => Ok(IpcResponse::success(
            context.storage.room_messages(&room_id).await?,
        )),
        IpcRequest::ListRooms => Ok(IpcResponse::success(
            context.storage.room_summaries("user").await?,
        )),
        IpcRequest::MessagePage {
            room_id,
            anchor,
            limit,
        } => {
            let anchor = match anchor {
                Some(anchor) => anchor,
                None => {
                    let summary = context
                        .storage
                        .room_summaries("user")
                        .await?
                        .into_iter()
                        .find(|room| room.id == room_id)
                        .ok_or_else(|| {
                            DaemonError::InvalidRequest(format!(
                                "user is not a member of room {room_id}"
                            ))
                        })?;
                    MessagePageAnchor::Around(summary.last_read_sequence.max(1))
                }
            };
            Ok(IpcResponse::success(
                context
                    .storage
                    .message_page(&room_id, MessagePageQuery { anchor, limit })
                    .await?,
            ))
        }
        IpcRequest::MarkRead {
            room_id,
            through_sequence,
        } => {
            let sequence = context
                .storage
                .mark_user_read(&room_id, through_sequence)
                .await?;
            context
                .events
                .publish(CollabEventKind::RoomsChanged { room_id })
                .await;
            Ok(IpcResponse::success(json!({"lastReadSequence": sequence})))
        }
        IpcRequest::Permissions => Ok(IpcResponse::success(context.permissions.snapshot().await)),
        IpcRequest::ReplyPermission { id, reply, message } => {
            let pending = context.permissions.get(&id).await.ok_or_else(|| {
                DaemonError::InvalidRequest(format!("permission {id} is no longer pending"))
            })?;
            let directory = pending.directory.as_deref().ok_or_else(|| {
                DaemonError::InvalidRequest(format!("permission {id} has no Agent directory"))
            })?;
            let connection = context
                .engine
                .borrow()
                .clone()
                .ok_or(OpenCodeError::Unavailable)?;
            let accepted = connection
                .client
                .reply_permission(Path::new(directory), &id, reply, message.as_deref())
                .await?;
            if accepted {
                context.permissions.remove(&id).await;
                context
                    .events
                    .publish(CollabEventKind::PermissionsChanged)
                    .await;
            }
            Ok(IpcResponse::success(json!({"accepted": accepted})))
        }
        IpcRequest::AbortPermission { id } => {
            let pending = context.permissions.get(&id).await.ok_or_else(|| {
                DaemonError::InvalidRequest(format!("permission {id} is no longer pending"))
            })?;
            let directory = pending.directory.as_deref().ok_or_else(|| {
                DaemonError::InvalidRequest(format!("permission {id} has no Agent directory"))
            })?;
            let connection = context
                .engine
                .borrow()
                .clone()
                .ok_or(OpenCodeError::Unavailable)?;
            let aborted = connection
                .client
                .abort(Path::new(directory), &pending.session_id)
                .await?;
            if aborted {
                context
                    .permissions
                    .remove_session(&pending.session_id)
                    .await;
                context
                    .events
                    .publish(CollabEventKind::PermissionsChanged)
                    .await;
            }
            Ok(IpcResponse::success(json!({"aborted": aborted})))
        }
        IpcRequest::SubscribeEvents { .. } => Err(DaemonError::InvalidRequest(
            "event subscriptions must use a streaming connection".to_string(),
        )),
        IpcRequest::CredentialCheck { provider_id } => {
            let store = PostgresCredentialStore::from_env(context.storage.pool().clone())
                .map_err(|error| DaemonError::Credential(error.to_string()))?;
            let credential = store
                .load(&provider_id)
                .await
                .map_err(|error| DaemonError::Credential(error.to_string()))?
                .ok_or_else(|| {
                    DaemonError::Credential(format!("provider {provider_id} not found"))
                })?;
            let kind = ProviderKind::parse(&credential.provider_kind).ok_or_else(|| {
                DaemonError::Credential(format!(
                    "provider {provider_id} has invalid kind {}",
                    credential.provider_kind
                ))
            })?;
            let response = json!({
                "providerId": credential.provider_id,
                "baseUrl": credential.base_url,
                "kind": kind.as_str(),
                "decrypted": !credential.api_key().is_empty(),
            });
            drop(credential);
            Ok(IpcResponse::success(response))
        }
        IpcRequest::ConfigureTriage {
            provider_id,
            model_id,
        } => Ok(IpcResponse::success(
            context
                .storage
                .configure_triage(&provider_id, &model_id)
                .await?,
        )),
        IpcRequest::ListTriages { room_id } => Ok(IpcResponse::success(
            context.storage.triage_records(room_id.as_deref()).await?,
        )),
        IpcRequest::ListLogs { room_id, limit } => Ok(IpcResponse::success(
            context
                .storage
                .log_entries(room_id.as_deref(), limit)
                .await?,
        )),
        IpcRequest::CreateBoard { id, room_id, title } => Ok(IpcResponse::success(
            context.storage.create_board(&id, &room_id, &title).await?,
        )),
        IpcRequest::CreateBoardColumn {
            id,
            board_id,
            title,
            position,
            is_done,
        } => Ok(IpcResponse::success(
            context
                .storage
                .create_board_column(&id, &board_id, &title, position, is_done)
                .await?,
        )),
        IpcRequest::ListBoards { room_id } => Ok(IpcResponse::success(
            context.storage.boards(&room_id).await?,
        )),
        IpcRequest::CreateCard { card } => {
            let mutation = context.storage.create_card(card, "user").await?;
            card_events::publish(&context.notices, &context.events, &mutation).await;
            Ok(IpcResponse::success(mutation))
        }
        IpcRequest::MoveCard {
            card_id,
            column_id,
            position,
        } => {
            let mutation = context
                .storage
                .move_card(&card_id, &column_id, position, "user")
                .await?;
            card_events::publish(&context.notices, &context.events, &mutation).await;
            Ok(IpcResponse::success(mutation))
        }
        IpcRequest::ReleaseCardClaim {
            card_id,
            claimed_by,
        } => {
            let mutation = context
                .storage
                .release_card_claim(&card_id, &claimed_by, "user", "user_cancelled")
                .await?
                .ok_or_else(|| {
                    DaemonError::InvalidRequest(format!(
                        "card {card_id} is no longer claimed by {claimed_by}"
                    ))
                })?;
            card_events::publish(&context.notices, &context.events, &mutation).await;
            Ok(IpcResponse::success(mutation))
        }
    }
}

pub async fn request(socket: &Path, request: &IpcRequest) -> Result<IpcResponse, DaemonError> {
    let mut stream = UnixStream::connect(socket)
        .await
        .map_err(|error| DaemonError::Connect {
            path: socket.to_path_buf(),
            source: error,
        })?;
    let mut encoded = serde_json::to_vec(request)?;
    encoded.push(b'\n');
    stream.write_all(&encoded).await?;
    stream.shutdown().await?;
    let mut response = String::new();
    BufReader::new(stream).read_line(&mut response).await?;
    Ok(serde_json::from_str(&response)?)
}

#[derive(Debug)]
pub struct SocketLock {
    listener: UnixListener,
    path: PathBuf,
}

impl Drop for SocketLock {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

pub async fn bind_single_instance(path: &Path) -> Result<SocketLock, DaemonError> {
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    match UnixListener::bind(path) {
        Ok(listener) => Ok(SocketLock {
            listener,
            path: path.to_path_buf(),
        }),
        Err(error) if error.kind() == std::io::ErrorKind::AddrInUse => {
            if UnixStream::connect(path).await.is_ok() {
                return Err(DaemonError::AlreadyRunning(path.to_path_buf()));
            }
            tokio::fs::remove_file(path).await?;
            UnixListener::bind(path)
                .map(|listener| SocketLock {
                    listener,
                    path: path.to_path_buf(),
                })
                .map_err(|retry| {
                    if retry.kind() == std::io::ErrorKind::AddrInUse {
                        DaemonError::AlreadyRunning(path.to_path_buf())
                    } else {
                        DaemonError::Io(retry)
                    }
                })
        }
        Err(error) => Err(DaemonError::Io(error)),
    }
}

#[derive(Debug, Error)]
pub enum DaemonError {
    #[error("openwork collaboration daemon is already running; socket is occupied: {}", .0.display())]
    AlreadyRunning(PathBuf),
    #[error("cannot connect to collaboration daemon at {}: {source}", path.display())]
    Connect {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("HOME is unavailable; set {COLLAB_HOME_ENV} explicitly")]
    MissingHome,
    #[error("invalid environment variable {name}={value:?}: {reason}")]
    InvalidEnvironment {
        name: &'static str,
        value: String,
        reason: String,
    },
    #[error("invalid daemon request: {0}")]
    InvalidRequest(String),
    #[error("credential access failed: {0}")]
    Credential(String),
    #[error("identity registry failed: {0}")]
    Identity(String),
    #[error(transparent)]
    Storage(#[from] StorageError),
    #[error(transparent)]
    Home(#[from] HomeError),
    #[error(transparent)]
    OpenCode(#[from] OpenCodeError),
    #[error("daemon I/O failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("daemon JSON failed: {0}")]
    Json(#[from] serde_json::Error),
}
