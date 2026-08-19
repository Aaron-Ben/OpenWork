use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    time::Duration,
};

use tokio::{
    sync::{mpsc, oneshot, watch},
    task::JoinHandle,
};
use tokio_util::sync::CancellationToken;

use crate::{
    activity::AgentRuntimeRegistry,
    event::{CollabEventKind, CollabEventPublisher},
    opencode::{EngineConnection, GlobalEvent, InstanceEventStream, OpenCodeClient},
};

struct SubscriptionRequest {
    agent_id: String,
    directory: PathBuf,
    ready: oneshot::Sender<Result<(), String>>,
}

#[derive(Clone)]
pub struct InstanceEventSubscriptions {
    requests: mpsc::UnboundedSender<SubscriptionRequest>,
}

impl InstanceEventSubscriptions {
    pub async fn ensure(&self, agent_id: &str, directory: PathBuf) -> Result<(), String> {
        let (ready, response) = oneshot::channel();
        self.requests
            .send(SubscriptionRequest {
                agent_id: agent_id.to_string(),
                directory,
                ready,
            })
            .map_err(|_| "instance event supervisor stopped".to_string())?;
        response
            .await
            .map_err(|_| "instance event supervisor dropped the subscription".to_string())?
    }
}

pub fn start(
    connections: watch::Receiver<Option<EngineConnection>>,
    events: mpsc::UnboundedSender<GlobalEvent>,
    published_events: CollabEventPublisher,
    runtime: AgentRuntimeRegistry,
    cancel: CancellationToken,
) -> (InstanceEventSubscriptions, JoinHandle<()>) {
    let (request_tx, mut request_rx) = mpsc::unbounded_channel::<SubscriptionRequest>();
    let subscriptions = InstanceEventSubscriptions {
        requests: request_tx,
    };
    let task = tokio::spawn(async move {
        let mut connections = connections;
        let mut directories = HashMap::<String, PathBuf>::new();
        let mut active = HashMap::<String, (u64, PathBuf, JoinHandle<()>)>::new();
        let (stream_tx, mut stream_rx) = mpsc::unbounded_channel();
        loop {
            tokio::select! {
                _ = cancel.cancelled() => break,
                request = request_rx.recv() => {
                    let Some(request) = request else { break };
                    directories.insert(request.agent_id.clone(), request.directory.clone());
                    let connection = connections.borrow().clone();
                    let result = match connection {
                        Some(connection) => ensure_active(
                            &mut active,
                            &request.agent_id,
                            &request.directory,
                            &connection,
                            &stream_tx,
                            &cancel,
                        ).await,
                        None => Err("OpenCode is restarting".to_string()),
                    };
                    let _ = request.ready.send(result);
                }
                changed = connections.changed() => {
                    if changed.is_err() { break; }
                    for (_, (_, _, task)) in active.drain() {
                        task.abort();
                    }
                    let connection = connections.borrow().clone();
                    if let Some(connection) = connection {
                        for (agent_id, directory) in &directories {
                            if let Err(error) = ensure_active(
                                &mut active,
                                agent_id,
                                directory,
                                &connection,
                                &stream_tx,
                                &cancel,
                            ).await {
                                eprintln!("failed to restore /event for Agent {agent_id}: {error}");
                            }
                        }
                    }
                }
                event = stream_rx.recv() => {
                    let Some(event) = event else { break };
                    if let Some((agent_id, activity)) = runtime.observe(&event).await {
                        published_events.publish(CollabEventKind::AgentActivityChanged {
                            agent_id,
                            activity,
                        }).await;
                    }
                    if events.send(event).is_err() { break; }
                }
            }
        }
        for (_, (_, _, task)) in active {
            task.abort();
        }
    });
    (subscriptions, task)
}

async fn ensure_active(
    active: &mut HashMap<String, (u64, PathBuf, JoinHandle<()>)>,
    agent_id: &str,
    directory: &Path,
    connection: &EngineConnection,
    events: &mpsc::UnboundedSender<GlobalEvent>,
    cancel: &CancellationToken,
) -> Result<(), String> {
    if active
        .get(agent_id)
        .is_some_and(|(generation, current, task)| {
            *generation == connection.generation && current == directory && !task.is_finished()
        })
    {
        return Ok(());
    }
    if let Some((_, _, task)) = active.remove(agent_id) {
        task.abort();
    }
    let stream = tokio::time::timeout(Duration::from_secs(10), connection.client.events(directory))
        .await
        .map_err(|_| format!("timed out connecting /event for {}", directory.display()))?
        .map_err(|error| error.to_string())?;
    let task = tokio::spawn(forward_instance(
        connection.client.clone(),
        directory.to_path_buf(),
        stream,
        events.clone(),
        cancel.clone(),
    ));
    active.insert(
        agent_id.to_string(),
        (connection.generation, directory.to_path_buf(), task),
    );
    Ok(())
}

async fn forward_instance(
    client: OpenCodeClient,
    directory: PathBuf,
    mut stream: InstanceEventStream,
    events: mpsc::UnboundedSender<GlobalEvent>,
    cancel: CancellationToken,
) {
    loop {
        tokio::select! {
            _ = cancel.cancelled() => return,
            event = stream.next() => match event {
                Ok(event) => {
                    if events.send(event).is_err() { return; }
                }
                Err(error) => {
                    eprintln!("Agent /event disconnected for {}: {error}", directory.display());
                    tokio::select! {
                        _ = cancel.cancelled() => return,
                        _ = tokio::time::sleep(Duration::from_millis(500)) => {}
                    }
                    match client.events(&directory).await {
                        Ok(next) => stream = next,
                        Err(error) => {
                            eprintln!("failed to reconnect Agent /event for {}: {error}", directory.display());
                        }
                    }
                }
            }
        }
    }
}
