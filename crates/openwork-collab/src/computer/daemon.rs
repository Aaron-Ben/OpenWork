use std::{collections::HashMap, path::PathBuf, sync::Arc, time::Duration};

use tokio::{
    sync::{mpsc, watch},
    task::JoinHandle,
};
use tokio_util::sync::CancellationToken;

use crate::protocol::{
    AgentAssignment, COLLAB_PROTOCOL_VERSION, ComputerStatus, EngineInventoryView, EngineStatus,
    HeartbeatRequest,
};

use super::{
    client::{DeviceClient, RuntimeClientError},
    engine::{
        EngineAdapter, EngineAvailability, EngineId, EngineIdError, EngineInventory,
        EngineRegistry, EngineRuntimeConfig,
    },
    home::{HomeError, HomeManager},
    runner::{AgentRunner, RunnerEngine, RunnerError, RunnerIdentity},
    scheduling::RunnerResources,
};

#[derive(Clone, Debug)]
pub struct ComputerOptions {
    pub state_root: PathBuf,
    pub runtime_base_url: String,
    pub device_token: String,
    pub shim_executable: PathBuf,
    pub supervised: bool,
    pub poll_interval: Duration,
    pub roster_interval: Duration,
    pub heartbeat_interval: Duration,
    pub engine_rescan_interval: Duration,
}

pub struct ComputerDaemon {
    options: ComputerOptions,
    engines: EngineRegistry,
}

struct RunnerHandle {
    assignment: AgentAssignment,
    shutdown: CancellationToken,
    task: JoinHandle<Result<(), RunnerError>>,
}

enum DaemonEvent {
    Roster(Vec<AgentAssignment>),
    Inventory(EngineInventory),
    Fatal(RuntimeClientError),
}

impl ComputerDaemon {
    pub fn new(options: ComputerOptions, engines: EngineRegistry) -> Self {
        Self { options, engines }
    }

    pub async fn run(self, shutdown: CancellationToken) -> Result<(), ComputerError> {
        let client = DeviceClient::new(
            self.options.runtime_base_url.clone(),
            self.options.device_token.clone(),
        );
        let inventory_adapter = self.engines.require(&EngineId::opencode())?;
        let inventory = inventory_adapter.probe().await?;
        let generation = client.start().await?;
        client
            .heartbeat(&heartbeat_request(
                generation,
                self.options.supervised,
                ComputerStatus::Online,
                &inventory,
            ))
            .await?;
        let home_manager = HomeManager::new(
            self.options.state_root,
            self.options.shim_executable,
            self.options.runtime_base_url,
        );
        let resources = RunnerResources::local();
        let runner_factory = RunnerFactory {
            client: &client,
            generation,
            home_manager: &home_manager,
            engines: &self.engines,
            poll_interval: self.options.poll_interval,
            resources: &resources,
        };
        let mut runners = HashMap::<String, RunnerHandle>::new();
        if inventory.availability == EngineAvailability::Available {
            match client.roster(generation).await {
                Ok(assignments) => runner_factory.reconcile(assignments, &mut runners).await?,
                Err(error) if error.is_terminal_identity_error() => return Err(error.into()),
                Err(error) => tracing::warn!(
                    %error,
                    "initial Agent roster sync failed; background loop will retry"
                ),
            }
        }

        let background_shutdown = shutdown.child_token();
        let (events_tx, mut events_rx) = mpsc::channel(16);
        let (inventory_tx, inventory_rx) = watch::channel(inventory.clone());
        let heartbeat_task = tokio::spawn(heartbeat_loop(
            client.clone(),
            generation,
            self.options.supervised,
            inventory_rx,
            self.options.heartbeat_interval,
            events_tx.clone(),
            background_shutdown.clone(),
        ));
        let roster_task = tokio::spawn(roster_loop(
            client.clone(),
            generation,
            self.options.roster_interval,
            events_tx.clone(),
            background_shutdown.clone(),
        ));
        let inventory_task = tokio::spawn(engine_rescan_loop(
            inventory_adapter,
            inventory_tx,
            self.options.engine_rescan_interval,
            events_tx,
            background_shutdown.clone(),
        ));
        let mut current_inventory = inventory;
        let mut runner_tick = tokio::time::interval(Duration::from_secs(1));
        runner_tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        let result = 'supervisor: loop {
            tokio::select! {
                _ = shutdown.cancelled() => break Ok(()),
                event = events_rx.recv() => match event {
                    Some(DaemonEvent::Roster(assignments)) if current_inventory.availability == EngineAvailability::Available => {
                        if let Err(error) = runner_factory.reconcile(assignments, &mut runners).await {
                            break Err(error);
                        }
                    }
                    Some(DaemonEvent::Roster(_)) => {}
                    Some(DaemonEvent::Inventory(inventory)) => {
                        current_inventory = inventory;
                        if current_inventory.availability == EngineAvailability::Missing
                            && let Err(error) = stop_all(&mut runners).await
                        {
                            break Err(error);
                        }
                    }
                    Some(DaemonEvent::Fatal(error)) => break Err(error.into()),
                    None if shutdown.is_cancelled() => break Ok(()),
                    None => break Err(ComputerError::BackgroundStopped),
                },
                _ = runner_tick.tick() => {
                    let finished: Vec<String> = runners
                        .iter()
                        .filter(|(_, handle)| handle.task.is_finished())
                        .map(|(id, _)| id.clone())
                        .collect();
                    for id in finished {
                        let handle = runners.remove(&id).expect("finished Runner still exists");
                        match handle.task.await {
                            Ok(Ok(())) => tracing::warn!(agent_id = id, "Agent Runner stopped unexpectedly"),
                            Ok(Err(error)) if error.is_fenced() => break 'supervisor Err(error.into()),
                            Ok(Err(error)) => tracing::warn!(agent_id = id, %error, "Agent Runner failed; roster loop will rebuild it"),
                            Err(error) => break 'supervisor Err(error.into()),
                        }
                    }
                }
            }
        };

        background_shutdown.cancel();
        let _ = heartbeat_task.await;
        let _ = roster_task.await;
        let _ = inventory_task.await;
        let stop_result = stop_all(&mut runners).await;
        if let Err(error) = client
            .heartbeat(&heartbeat_request(
                generation,
                self.options.supervised,
                ComputerStatus::Offline,
                &current_inventory,
            ))
            .await
        {
            tracing::warn!(%error, "final Local Computer offline heartbeat failed");
        }
        result.and(stop_result)
    }
}

async fn heartbeat_loop(
    client: DeviceClient,
    generation: i64,
    supervised: bool,
    inventory: watch::Receiver<EngineInventory>,
    interval: Duration,
    events: mpsc::Sender<DaemonEvent>,
    shutdown: CancellationToken,
) {
    let mut delay = interval;
    loop {
        if !wait(delay, &shutdown).await {
            return;
        }
        let current_inventory = inventory.borrow().clone();
        let request = heartbeat_request(
            generation,
            supervised,
            ComputerStatus::Online,
            &current_inventory,
        );
        match client.heartbeat(&request).await {
            Ok(()) => delay = interval,
            Err(error) if error.is_terminal_identity_error() => {
                let _ = events.send(DaemonEvent::Fatal(error)).await;
                return;
            }
            Err(error) => {
                tracing::warn!(%error, "Local Computer heartbeat failed; retrying with backoff");
                delay = next_backoff(delay, interval);
            }
        }
    }
}

async fn roster_loop(
    client: DeviceClient,
    generation: i64,
    interval: Duration,
    events: mpsc::Sender<DaemonEvent>,
    shutdown: CancellationToken,
) {
    let mut delay = interval;
    loop {
        if !wait(delay, &shutdown).await {
            return;
        }
        match client.roster(generation).await {
            Ok(assignments) => {
                delay = interval;
                if events.send(DaemonEvent::Roster(assignments)).await.is_err() {
                    return;
                }
            }
            Err(error) if error.is_terminal_identity_error() => {
                let _ = events.send(DaemonEvent::Fatal(error)).await;
                return;
            }
            Err(error) => {
                tracing::warn!(%error, "Agent roster sync failed; preserving current Runners");
                delay = next_backoff(delay, interval);
            }
        }
    }
}

async fn engine_rescan_loop(
    adapter: Arc<dyn EngineAdapter>,
    inventories: watch::Sender<EngineInventory>,
    interval: Duration,
    events: mpsc::Sender<DaemonEvent>,
    shutdown: CancellationToken,
) {
    let mut delay = interval;
    loop {
        if !wait(delay, &shutdown).await {
            return;
        }
        match adapter.probe().await {
            Ok(inventory) => {
                delay = interval;
                inventories.send_replace(inventory.clone());
                if events
                    .send(DaemonEvent::Inventory(inventory))
                    .await
                    .is_err()
                {
                    return;
                }
            }
            Err(error) => {
                tracing::warn!(%error, "Engine inventory scan failed; preserving the last reliable result");
                delay = next_backoff(delay, interval);
            }
        }
    }
}

async fn wait(delay: Duration, shutdown: &CancellationToken) -> bool {
    tokio::select! {
        _ = shutdown.cancelled() => false,
        _ = tokio::time::sleep(delay) => true,
    }
}

fn next_backoff(current: Duration, base: Duration) -> Duration {
    (current * 2).max(base).min(Duration::from_secs(5 * 60))
}

fn heartbeat_request(
    generation: i64,
    supervised: bool,
    status: ComputerStatus,
    inventory: &EngineInventory,
) -> HeartbeatRequest {
    HeartbeatRequest {
        protocol_version: COLLAB_PROTOCOL_VERSION,
        generation,
        daemon_version: env!("CARGO_PKG_VERSION").to_string(),
        supervised,
        status,
        engine: EngineInventoryView {
            engine_id: EngineId::opencode().to_string(),
            status: match inventory.availability {
                EngineAvailability::Available => EngineStatus::Ready,
                EngineAvailability::Missing => EngineStatus::Missing,
            },
        },
    }
}

async fn stop_all(runners: &mut HashMap<String, RunnerHandle>) -> Result<(), ComputerError> {
    for handle in runners.values() {
        handle.shutdown.cancel();
    }
    for (_, handle) in runners.drain() {
        handle.task.await??;
    }
    Ok(())
}

struct RunnerFactory<'a> {
    client: &'a DeviceClient,
    generation: i64,
    home_manager: &'a HomeManager,
    engines: &'a EngineRegistry,
    poll_interval: Duration,
    resources: &'a RunnerResources,
}

impl RunnerFactory<'_> {
    async fn reconcile(
        &self,
        assignments: Vec<AgentAssignment>,
        runners: &mut HashMap<String, RunnerHandle>,
    ) -> Result<(), ComputerError> {
        let desired: HashMap<&str, &AgentAssignment> = assignments
            .iter()
            .map(|assignment| (assignment.id.as_str(), assignment))
            .collect();
        let removed: Vec<String> = runners
            .keys()
            .filter(|id| {
                desired
                    .get(id.as_str())
                    .is_none_or(|assignment| **assignment != runners[*id].assignment)
            })
            .cloned()
            .collect();
        for id in removed {
            if let Some(handle) = runners.remove(&id) {
                handle.shutdown.cancel();
                handle.task.await??;
            }
        }
        for assignment in assignments {
            if !runners.contains_key(&assignment.id) {
                self.start(assignment, runners).await?;
            }
        }
        Ok(())
    }

    async fn start(
        &self,
        assignment: AgentAssignment,
        runners: &mut HashMap<String, RunnerHandle>,
    ) -> Result<(), ComputerError> {
        let engine_id = EngineId::new(assignment.engine_id.clone())?;
        let adapter = self.engines.require(&engine_id)?;
        let runtime_token = self
            .client
            .mint_agent_token(&assignment.id, self.generation)
            .await?;
        let home = self
            .home_manager
            .materialize(&assignment, &runtime_token.token)
            .await?;
        let engine_runtime = adapter
            .create_agent_runtime(EngineRuntimeConfig {
                home: home.root.clone(),
                config_root: home.config_root.clone(),
                state_file: home.state_file.clone(),
                config_fingerprint: home.config_fingerprint.clone(),
                model: assignment.model.clone(),
                environment: home.environment.clone(),
            })
            .await?;
        let runner_shutdown = CancellationToken::new();
        let task_shutdown = runner_shutdown.clone();
        let id = assignment.id.clone();
        let handle_assignment = assignment.clone();
        let runner = AgentRunner::new(
            assignment,
            RunnerIdentity {
                device: self.client.clone(),
                generation: self.generation,
                token_expires_at: runtime_token.expires_at,
            },
            self.client.agent(runtime_token.token),
            RunnerEngine {
                adapter,
                runtime: engine_runtime,
            },
            home,
            self.poll_interval,
            self.resources.clone(),
        );
        let task = tokio::spawn(async move { runner.run(task_shutdown).await });
        runners.insert(
            id,
            RunnerHandle {
                assignment: handle_assignment,
                shutdown: runner_shutdown,
                task,
            },
        );
        Ok(())
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ComputerError {
    #[error(transparent)]
    Runtime(#[from] RuntimeClientError),
    #[error(transparent)]
    Engine(#[from] super::engine::EngineError),
    #[error(transparent)]
    EngineId(#[from] EngineIdError),
    #[error(transparent)]
    Home(#[from] HomeError),
    #[error(transparent)]
    Runner(#[from] RunnerError),
    #[error("Agent runner task failed: {0}")]
    Task(#[from] tokio::task::JoinError),
    #[error("Computer background loop stopped unexpectedly")]
    BackgroundStopped,
}
