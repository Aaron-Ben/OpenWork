use std::{collections::HashMap, path::PathBuf, sync::Arc, time::Duration};

use tokio::{sync::mpsc, task::JoinHandle};
use tokio_util::sync::CancellationToken;

use crate::protocol::{AgentAssignment, EngineInventoryReport, EngineObservation, EngineStatus};

use super::{
    client::{ComputerClient, RuntimeClientError},
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
    pub runtime_session_id: String,
    pub runtime_base_url: String,
    pub computer_secret: String,
    pub shim_executable: PathBuf,
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
        let client = ComputerClient::new(
            self.options.runtime_base_url.clone(),
            self.options.runtime_session_id.clone(),
            self.options.computer_secret.clone(),
        );
        let inventory_adapter = self.engines.require(&EngineId::opencode())?;
        let inventory = inventory_adapter.probe().await?;
        client
            .report_inventory(&inventory_report(&inventory))
            .await?;
        client.heartbeat(Vec::new()).await?;
        let home_manager = HomeManager::new(
            self.options.state_root,
            self.options.shim_executable,
            self.options.runtime_base_url,
        );
        let resources = RunnerResources::local();
        let runner_factory = RunnerFactory {
            client: &client,
            home_manager: &home_manager,
            engines: &self.engines,
            poll_interval: self.options.poll_interval,
            resources: &resources,
        };
        let mut runners = HashMap::<String, RunnerHandle>::new();
        if inventory.availability == EngineAvailability::Available {
            let snapshot = client.desired_agents().await?;
            runner_factory
                .reconcile(snapshot.agents, &mut runners)
                .await?;
        }

        let background_shutdown = shutdown.child_token();
        let (events_tx, mut events_rx) = mpsc::channel(16);
        let (management_tx, management_rx) = mpsc::channel(1);
        let management_client = client.clone();
        let management_shutdown = background_shutdown.clone();
        let management_task = tokio::spawn(async move {
            management_client
                .management_loop(management_tx, management_shutdown)
                .await;
        });
        let heartbeat_task = tokio::spawn(heartbeat_loop(
            client.clone(),
            self.options.heartbeat_interval,
            events_tx.clone(),
            background_shutdown.clone(),
        ));
        let roster_task = tokio::spawn(roster_loop(
            client.clone(),
            self.options.roster_interval,
            management_rx,
            events_tx.clone(),
            background_shutdown.clone(),
        ));
        let inventory_task = tokio::spawn(engine_rescan_loop(
            client.clone(),
            inventory_adapter,
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
                    Some(DaemonEvent::Roster(assignments))
                        if current_inventory.availability == EngineAvailability::Available =>
                    {
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
                    let finished = runners
                        .iter()
                        .filter(|(_, handle)| handle.task.is_finished())
                        .map(|(id, _)| id.clone())
                        .collect::<Vec<_>>();
                    for id in finished {
                        let handle = runners.remove(&id).expect("finished Runner still exists");
                        match handle.task.await {
                            Ok(Ok(())) => tracing::warn!(agent_id = id, "Agent Runner stopped unexpectedly"),
                            Ok(Err(error)) if error.is_fenced() => break 'supervisor Err(error.into()),
                            Ok(Err(error)) => tracing::warn!(agent_id = id, %error, "Agent Runner failed; reconcile will rebuild it"),
                            Err(error) => break 'supervisor Err(error.into()),
                        }
                    }
                }
            }
        };

        background_shutdown.cancel();
        let _ = management_task.await;
        let _ = heartbeat_task.await;
        let _ = roster_task.await;
        let _ = inventory_task.await;
        let stop_result = stop_all(&mut runners).await;
        result.and(stop_result)
    }
}

async fn heartbeat_loop(
    client: ComputerClient,
    interval: Duration,
    events: mpsc::Sender<DaemonEvent>,
    shutdown: CancellationToken,
) {
    let mut delay = interval;
    loop {
        if !wait(delay, &shutdown).await {
            return;
        }
        match client.heartbeat(Vec::new()).await {
            Ok(()) => delay = interval,
            Err(error) if error.is_terminal_identity_error() => {
                let _ = events.send(DaemonEvent::Fatal(error)).await;
                return;
            }
            Err(error) => {
                tracing::warn!(%error, "Computer heartbeat failed; retrying with backoff");
                delay = next_backoff(delay, interval);
            }
        }
    }
}

async fn roster_loop(
    client: ComputerClient,
    interval: Duration,
    mut invalidations: mpsc::Receiver<()>,
    events: mpsc::Sender<DaemonEvent>,
    shutdown: CancellationToken,
) {
    let mut retry_delay = interval;
    loop {
        tokio::select! {
            _ = shutdown.cancelled() => return,
            _ = tokio::time::sleep(retry_delay) => {}
            invalidation = invalidations.recv() => {
                if invalidation.is_none() && shutdown.is_cancelled() {
                    return;
                }
            }
        }
        match client.desired_agents().await {
            Ok(snapshot) => {
                retry_delay = interval;
                if events
                    .send(DaemonEvent::Roster(snapshot.agents))
                    .await
                    .is_err()
                {
                    return;
                }
            }
            Err(error) if error.is_terminal_identity_error() => {
                let _ = events.send(DaemonEvent::Fatal(error)).await;
                return;
            }
            Err(error) => {
                tracing::warn!(%error, "desired Agent snapshot failed; preserving current Runners");
                retry_delay = next_backoff(retry_delay, interval);
            }
        }
    }
}

async fn engine_rescan_loop(
    client: ComputerClient,
    adapter: Arc<dyn EngineAdapter>,
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
                if let Err(error) = client.report_inventory(&inventory_report(&inventory)).await {
                    if error.is_terminal_identity_error() {
                        let _ = events.send(DaemonEvent::Fatal(error)).await;
                        return;
                    }
                    tracing::warn!(%error, "Engine observation report failed");
                }
                if events
                    .send(DaemonEvent::Inventory(inventory))
                    .await
                    .is_err()
                {
                    return;
                }
            }
            Err(error) => {
                tracing::warn!(%error, "Engine scan failed; preserving last observation");
                delay = next_backoff(delay, interval);
            }
        }
    }
}

fn inventory_report(inventory: &EngineInventory) -> EngineInventoryReport {
    EngineInventoryReport {
        engines: vec![EngineObservation {
            engine_id: EngineId::opencode().to_string(),
            status: match inventory.availability {
                EngineAvailability::Available => EngineStatus::Ready,
                EngineAvailability::Missing => EngineStatus::Missing,
            },
            version: None,
            checked_at: time::OffsetDateTime::now_utc().unix_timestamp(),
            last_error: None,
        }],
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

async fn stop_all(runners: &mut HashMap<String, RunnerHandle>) -> Result<(), ComputerError> {
    for handle in runners.values() {
        handle.shutdown.cancel();
    }
    let deadline = tokio::time::Instant::now() + Duration::from_secs(15);
    let mut first_error = None;
    for (_, mut handle) in runners.drain() {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        match tokio::time::timeout(remaining, &mut handle.task).await {
            Ok(Ok(Ok(()))) => {}
            Ok(Ok(Err(error))) if first_error.is_none() => {
                first_error = Some(ComputerError::Runner(error));
            }
            Ok(Err(error)) if first_error.is_none() => {
                first_error = Some(ComputerError::Task(error));
            }
            Err(_) => {
                handle.task.abort();
                let _ = handle.task.await;
                if first_error.is_none() {
                    first_error = Some(ComputerError::ShutdownTimeout);
                }
            }
            _ => {}
        }
    }
    first_error.map_or(Ok(()), Err)
}

struct RunnerFactory<'a> {
    client: &'a ComputerClient,
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
        let desired = assignments
            .iter()
            .map(|assignment| (assignment.id.as_str(), assignment))
            .collect::<HashMap<_, _>>();
        let removed = runners
            .keys()
            .filter(|id| {
                desired
                    .get(id.as_str())
                    .is_none_or(|assignment| **assignment != runners[*id].assignment)
            })
            .cloned()
            .collect::<Vec<_>>();
        for id in removed {
            if let Some(handle) = runners.remove(&id) {
                handle.shutdown.cancel();
                handle.task.await??;
            }
        }
        for assignment in assignments {
            if !runners.contains_key(&assignment.id)
                && let Err(error) = self.start(assignment.clone(), runners).await
            {
                tracing::warn!(agent_id = assignment.id, %error, "Agent Runner could not start");
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
        let runtime_token = self.client.mint_agent_token(&assignment.id).await?;
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
                model: assignment.main_model_id.clone(),
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
                computer: self.client.clone(),
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
    #[error("Agent Runner task failed: {0}")]
    Task(#[from] tokio::task::JoinError),
    #[error("Computer background loop stopped unexpectedly")]
    BackgroundStopped,
    #[error("Computer exceeded the 15-second shutdown deadline")]
    ShutdownTimeout,
}
