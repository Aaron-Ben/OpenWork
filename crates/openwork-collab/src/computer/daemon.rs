use std::{collections::HashMap, path::PathBuf, sync::Arc, time::Duration};

use tokio::{
    sync::{mpsc, watch},
    task::JoinHandle,
};
use tokio_util::sync::CancellationToken;

use crate::protocol::{
    AgentAssignment, ComputerHeartbeatRequest, EngineInventoryReport, EngineObservation,
    EngineReadinessView, EngineStatus, RunnerState, RunnerStatusView,
};

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
    pub openwork_root: PathBuf,
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
    Inventory(EngineProbe),
    Fatal(RuntimeClientError),
}

#[derive(Clone, Debug)]
struct EngineProbe {
    readiness: EngineReadinessView,
    observation: EngineObservation,
}

impl EngineProbe {
    fn runnable(&self) -> bool {
        self.readiness.status == EngineStatus::Ready
    }
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
        let inventory = probe_engine(&inventory_adapter).await;
        client
            .report_inventory(&inventory_report(&inventory))
            .await?;
        let home_manager = HomeManager::prepare(
            self.options.openwork_root,
            &self.options.runtime_session_id,
            self.options.shim_executable,
            self.options.runtime_base_url,
        )
        .await?;
        let resources = RunnerResources::local();
        let runner_factory = RunnerFactory {
            client: &client,
            home_manager: &home_manager,
            engines: &self.engines,
            poll_interval: self.options.poll_interval,
            resources: &resources,
        };
        let mut runners = HashMap::<String, RunnerHandle>::new();
        let snapshot = client.desired_agents().await?;
        let runner_statuses = if inventory.runnable() {
            runner_factory
                .reconcile(snapshot.agents, &mut runners)
                .await?
        } else {
            Vec::new()
        };
        let initial_actual = actual_state(&inventory, runner_statuses);
        client.heartbeat(&initial_actual).await?;

        let background_shutdown = shutdown.child_token();
        let (events_tx, mut events_rx) = mpsc::channel(16);
        let (actual_tx, actual_rx) = watch::channel(initial_actual);
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
            actual_rx,
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
        let mut current_runner_statuses = actual_tx.borrow().runners.clone();
        let mut runner_tick = tokio::time::interval(Duration::from_secs(1));
        runner_tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        let result = 'supervisor: loop {
            tokio::select! {
                _ = shutdown.cancelled() => break Ok(()),
                event = events_rx.recv() => match event {
                    Some(DaemonEvent::Roster(assignments))
                        if current_inventory.runnable() =>
                    {
                        match runner_factory.reconcile(assignments, &mut runners).await {
                            Ok(statuses) => {
                                current_runner_statuses = statuses;
                                actual_tx.send_replace(actual_state(
                                    &current_inventory,
                                    current_runner_statuses.clone(),
                                ));
                            }
                            Err(error) => break Err(error),
                        }
                    }
                    Some(DaemonEvent::Roster(_)) => {}
                    Some(DaemonEvent::Inventory(inventory)) => {
                        current_inventory = inventory;
                        if current_inventory.runnable() {
                            match client.desired_agents().await {
                                Ok(snapshot) => match runner_factory
                                    .reconcile(snapshot.agents, &mut runners)
                                    .await
                                {
                                    Ok(statuses) => current_runner_statuses = statuses,
                                    Err(error) => break Err(error),
                                },
                                Err(error) if error.is_terminal_identity_error() => {
                                    break Err(error.into());
                                }
                                Err(error) => {
                                    tracing::warn!(%error, "Engine recovered but desired Agent snapshot failed");
                                }
                            }
                        } else {
                            if let Err(error) = stop_all(&mut runners).await {
                                tracing::warn!(%error, "Runners required forced stop after Engine became unavailable");
                            }
                            current_runner_statuses.clear();
                        }
                        actual_tx.send_replace(actual_state(
                            &current_inventory,
                            current_runner_statuses.clone(),
                        ));
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
                        let assignment = handle.assignment.clone();
                        match handle.task.await {
                            Ok(Ok(())) => {
                                tracing::warn!(agent_id = id, "Agent Runner stopped unexpectedly");
                                record_runner_error(
                                    &mut current_runner_statuses,
                                    &assignment,
                                    "Agent Runner stopped unexpectedly".to_string(),
                                );
                            }
                            Ok(Err(error)) if error.is_fenced() => break 'supervisor Err(error.into()),
                            Ok(Err(error)) => {
                                tracing::warn!(agent_id = id, %error, "Agent Runner failed; reconcile will rebuild it");
                                record_runner_error(
                                    &mut current_runner_statuses,
                                    &assignment,
                                    error.to_string(),
                                );
                            }
                            Err(error) => {
                                tracing::warn!(agent_id = id, %error, "Agent Runner task failed; reconcile will rebuild it");
                                record_runner_error(
                                    &mut current_runner_statuses,
                                    &assignment,
                                    error.to_string(),
                                );
                            }
                        }
                        actual_tx.send_replace(actual_state(
                            &current_inventory,
                            current_runner_statuses.clone(),
                        ));
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
    mut actual: watch::Receiver<ComputerHeartbeatRequest>,
    events: mpsc::Sender<DaemonEvent>,
    shutdown: CancellationToken,
) {
    let mut retry_delay = Duration::from_secs(1);
    loop {
        tokio::select! {
            _ = shutdown.cancelled() => return,
            changed = actual.changed() => {
                if changed.is_err() {
                    return;
                }
            }
            _ = tokio::time::sleep(interval) => {}
        }
        let state = actual.borrow_and_update().clone();
        match client.heartbeat(&state).await {
            Ok(()) => retry_delay = Duration::from_secs(1),
            Err(error) if error.is_terminal_identity_error() => {
                let _ = events.send(DaemonEvent::Fatal(error)).await;
                return;
            }
            Err(error) => {
                tracing::warn!(%error, "Computer heartbeat failed; retrying with backoff");
                if !wait(retry_delay, &shutdown).await {
                    return;
                }
                retry_delay = (retry_delay * 2).min(Duration::from_secs(30));
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
    loop {
        if !wait(interval, &shutdown).await {
            return;
        }
        let inventory = probe_engine(&adapter).await;
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
}

async fn probe_engine(adapter: &Arc<dyn EngineAdapter>) -> EngineProbe {
    let (status, last_error) = match adapter.probe().await {
        Ok(EngineInventory {
            availability: EngineAvailability::Available,
        }) => (EngineStatus::Ready, None),
        Ok(EngineInventory {
            availability: EngineAvailability::Missing,
        }) => (EngineStatus::Missing, None),
        Err(error) => {
            tracing::warn!(%error, "Engine scan failed");
            (EngineStatus::Error, Some(bounded_error(error.to_string())))
        }
    };
    let checked_at = time::OffsetDateTime::now_utc().unix_timestamp();
    EngineProbe {
        readiness: EngineReadinessView {
            engine_id: EngineId::opencode().to_string(),
            status,
        },
        observation: EngineObservation {
            engine_id: EngineId::opencode().to_string(),
            status,
            version: None,
            checked_at,
            last_error,
        },
    }
}

fn inventory_report(inventory: &EngineProbe) -> EngineInventoryReport {
    EngineInventoryReport {
        engines: vec![inventory.observation.clone()],
    }
}

fn actual_state(
    inventory: &EngineProbe,
    mut runners: Vec<RunnerStatusView>,
) -> ComputerHeartbeatRequest {
    runners.sort_by(|left, right| left.agent_id.cmp(&right.agent_id));
    ComputerHeartbeatRequest {
        engine_readiness: vec![inventory.readiness.clone()],
        runners,
    }
}

fn record_runner_error(
    statuses: &mut Vec<RunnerStatusView>,
    assignment: &AgentAssignment,
    error: String,
) {
    statuses.retain(|status| status.agent_id != assignment.id);
    statuses.push(RunnerStatusView {
        agent_id: assignment.id.clone(),
        config_revision: assignment.config_revision,
        state: RunnerState::Error,
        last_error: Some(bounded_error(error)),
    });
    statuses.sort_by(|left, right| left.agent_id.cmp(&right.agent_id));
}

fn bounded_error(error: String) -> String {
    const MAX_CHARS: usize = 1_000;
    if error.chars().count() <= MAX_CHARS {
        error
    } else {
        let mut bounded = error.chars().take(MAX_CHARS).collect::<String>();
        bounded.push('…');
        bounded
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

async fn stop_runner(mut handle: RunnerHandle) -> Result<(), ComputerError> {
    handle.shutdown.cancel();
    match tokio::time::timeout(Duration::from_secs(15), &mut handle.task).await {
        Ok(Ok(Ok(()))) => Ok(()),
        Ok(Ok(Err(error))) => Err(error.into()),
        Ok(Err(error)) => Err(error.into()),
        Err(_) => {
            handle.task.abort();
            let _ = handle.task.await;
            Err(ComputerError::ShutdownTimeout)
        }
    }
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
    ) -> Result<Vec<RunnerStatusView>, ComputerError> {
        let desired = assignments
            .iter()
            .map(|assignment| (assignment.id.as_str(), assignment))
            .collect::<HashMap<_, _>>();
        let mut failures = Vec::new();
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
            if let Some(handle) = runners.remove(&id)
                && let Err(error) = stop_runner(handle).await
            {
                tracing::warn!(agent_id = id, %error, "Agent Runner required forced replacement");
            }
        }
        for assignment in assignments {
            if !runners.contains_key(&assignment.id) {
                match self.start(assignment.clone(), runners).await {
                    Ok(()) => {}
                    Err(error) if error.is_fenced() => return Err(error),
                    Err(error) => {
                        tracing::warn!(agent_id = assignment.id, %error, "Agent Runner could not start");
                        failures.push(RunnerStatusView {
                            agent_id: assignment.id,
                            config_revision: assignment.config_revision,
                            state: RunnerState::Error,
                            last_error: Some(bounded_error(error.to_string())),
                        });
                    }
                }
            }
        }
        let mut statuses = runners
            .values()
            .map(|handle| RunnerStatusView {
                agent_id: handle.assignment.id.clone(),
                config_revision: handle.assignment.config_revision,
                state: RunnerState::Running,
                last_error: None,
            })
            .collect::<Vec<_>>();
        statuses.extend(failures);
        statuses.sort_by(|left, right| left.agent_id.cmp(&right.agent_id));
        Ok(statuses)
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
                home: home.work_root.clone(),
                config_root: home.config_root.clone(),
                state_file: home.state_file.clone(),
                context_fingerprint: home.context_fingerprint.clone(),
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

impl ComputerError {
    fn is_fenced(&self) -> bool {
        match self {
            Self::Runtime(error) => error.is_terminal_identity_error(),
            Self::Runner(error) => error.is_fenced(),
            _ => false,
        }
    }
}
