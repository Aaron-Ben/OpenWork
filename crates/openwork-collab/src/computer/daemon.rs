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

const RUNNER_SHUTDOWN_GRACE: Duration = Duration::from_secs(15);
const RUNNER_FORCE_REAP_TIMEOUT: Duration = Duration::from_secs(1);
const RUNNER_RESTART_BASE: Duration = Duration::from_secs(1);
const RUNNER_RESTART_MAX: Duration = Duration::from_secs(30);
const RUNNER_STABLE_AFTER: Duration = Duration::from_secs(60);

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
    stop_requested: CancellationToken,
    force_cancel: CancellationToken,
    task: JoinHandle<Result<(), RunnerError>>,
    started_at: tokio::time::Instant,
    restart_failures: u32,
}

#[derive(Clone, Copy)]
struct RestartPlan {
    failures: u32,
    not_before: tokio::time::Instant,
}

impl RestartPlan {
    fn after_failure(failures: u32) -> Self {
        let exponent = failures.saturating_sub(1).min(5);
        let delay = (RUNNER_RESTART_BASE * 2_u32.pow(exponent)).min(RUNNER_RESTART_MAX);
        Self {
            failures,
            not_before: tokio::time::Instant::now() + delay,
        }
    }
}

enum DaemonEvent {
    Roster(Vec<AgentAssignment>),
    Inventory(Vec<EngineProbe>),
    Fatal(RuntimeClientError),
    BackgroundStopped(&'static str),
}

#[derive(Clone, Debug)]
struct EngineProbe {
    readiness: EngineReadinessView,
    observation: EngineObservation,
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
        let inventory_adapters = self.engines.adapters();
        let inventory = probe_engines(&inventory_adapters).await;
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
        let mut restart_plans = HashMap::<String, RestartPlan>::new();
        let snapshot = client.desired_agents().await?;
        let runner_statuses = runner_factory
            .reconcile(
                snapshot.agents,
                &inventory,
                &mut runners,
                &mut restart_plans,
            )
            .await?;
        let initial_actual = actual_state(&inventory, runner_statuses);
        client.heartbeat(&initial_actual).await?;

        let background_shutdown = shutdown.child_token();
        let (events_tx, mut events_rx) = mpsc::channel(16);
        let (actual_tx, actual_rx) = watch::channel(initial_actual);
        let (management_tx, management_rx) = mpsc::channel(1);
        let management_client = client.clone();
        let management_shutdown = background_shutdown.clone();
        let management_events = events_tx.clone();
        let management_task = tokio::spawn(async move {
            management_client
                .management_loop(management_tx, management_shutdown.clone())
                .await;
            if !management_shutdown.is_cancelled() {
                let _ = management_events
                    .send(DaemonEvent::BackgroundStopped("management SSE"))
                    .await;
            }
        });
        let heartbeat_client = client.clone();
        let heartbeat_interval = self.options.heartbeat_interval;
        let heartbeat_events = events_tx.clone();
        let heartbeat_shutdown = background_shutdown.clone();
        let heartbeat_task = tokio::spawn(async move {
            heartbeat_loop(
                heartbeat_client,
                heartbeat_interval,
                actual_rx,
                heartbeat_events.clone(),
                heartbeat_shutdown.clone(),
            )
            .await;
            if !heartbeat_shutdown.is_cancelled() {
                let _ = heartbeat_events
                    .send(DaemonEvent::BackgroundStopped("heartbeat"))
                    .await;
            }
        });
        let roster_events = events_tx.clone();
        let roster_shutdown = background_shutdown.clone();
        let roster_client = client.clone();
        let roster_interval = self.options.roster_interval;
        let roster_task = tokio::spawn(async move {
            roster_loop(
                roster_client,
                roster_interval,
                management_rx,
                roster_events.clone(),
                roster_shutdown.clone(),
            )
            .await;
            if !roster_shutdown.is_cancelled() {
                let _ = roster_events
                    .send(DaemonEvent::BackgroundStopped("roster"))
                    .await;
            }
        });
        let inventory_events = events_tx.clone();
        let inventory_shutdown = background_shutdown.clone();
        let inventory_client = client.clone();
        let inventory_interval = self.options.engine_rescan_interval;
        let inventory_task = tokio::spawn(async move {
            engine_rescan_loop(
                inventory_client,
                inventory_adapters,
                inventory_interval,
                inventory_events.clone(),
                inventory_shutdown.clone(),
            )
            .await;
            if !inventory_shutdown.is_cancelled() {
                let _ = inventory_events
                    .send(DaemonEvent::BackgroundStopped("engine inventory"))
                    .await;
            }
        });
        let mut current_inventory = inventory;
        let mut current_runner_statuses = actual_tx.borrow().runners.clone();
        let mut runner_tick = tokio::time::interval(Duration::from_secs(1));
        runner_tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        let result = 'supervisor: loop {
            tokio::select! {
                _ = shutdown.cancelled() => break Ok(()),
                event = events_rx.recv() => match event {
                    Some(DaemonEvent::Roster(assignments)) => {
                        match runner_factory
                            .reconcile(
                                assignments,
                                &current_inventory,
                                &mut runners,
                                &mut restart_plans,
                            )
                            .await
                        {
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
                    Some(DaemonEvent::Inventory(inventory)) => {
                        current_inventory = inventory;
                        match client.desired_agents().await {
                            Ok(snapshot) => match runner_factory
                                .reconcile(
                                    snapshot.agents,
                                    &current_inventory,
                                    &mut runners,
                                    &mut restart_plans,
                                )
                                .await
                            {
                                Ok(statuses) => current_runner_statuses = statuses,
                                Err(error) => break Err(error),
                            },
                            Err(error) if error.is_terminal_identity_error() => {
                                break Err(error.into());
                            }
                            Err(error) => {
                                tracing::warn!(%error, "Engine inventory changed but desired Agent snapshot failed");
                            }
                        }
                        actual_tx.send_replace(actual_state(
                            &current_inventory,
                            current_runner_statuses.clone(),
                        ));
                    }
                    Some(DaemonEvent::Fatal(error)) => break Err(error.into()),
                    Some(DaemonEvent::BackgroundStopped(name)) if !background_shutdown.is_cancelled() => {
                        tracing::error!(loop_name = name, "Computer background loop stopped unexpectedly");
                        break Err(ComputerError::BackgroundStopped);
                    }
                    Some(DaemonEvent::BackgroundStopped(_)) => break Ok(()),
                    None if shutdown.is_cancelled() => break Ok(()),
                    None => break Err(ComputerError::BackgroundStopped),
                },
                _ = runner_tick.tick() => {
                    for handle in runners.values_mut() {
                        if handle.restart_failures > 0
                            && handle.started_at.elapsed() >= RUNNER_STABLE_AFTER
                        {
                            handle.restart_failures = 0;
                        }
                    }
                    let finished = runners
                        .iter()
                        .filter(|(_, handle)| handle.task.is_finished())
                        .map(|(id, _)| id.clone())
                        .collect::<Vec<_>>();
                    for id in finished {
                        let handle = runners.remove(&id).expect("finished Runner still exists");
                        let assignment = handle.assignment.clone();
                        let restart_failures = if handle.started_at.elapsed() >= RUNNER_STABLE_AFTER {
                            1
                        } else {
                            handle.restart_failures.saturating_add(1)
                        };
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
                        restart_plans.insert(id, RestartPlan::after_failure(restart_failures));
                        actual_tx.send_replace(actual_state(
                            &current_inventory,
                            current_runner_statuses.clone(),
                        ));
                    }
                    if restart_plans
                        .values()
                        .any(|plan| plan.not_before <= tokio::time::Instant::now())
                    {
                        match client.desired_agents().await {
                            Ok(snapshot) => match runner_factory
                                .reconcile(
                                    snapshot.agents,
                                    &current_inventory,
                                    &mut runners,
                                    &mut restart_plans,
                                )
                                .await
                            {
                                Ok(statuses) => {
                                    current_runner_statuses = statuses;
                                    actual_tx.send_replace(actual_state(
                                        &current_inventory,
                                        current_runner_statuses.clone(),
                                    ));
                                }
                                Err(error) => break Err(error),
                            },
                            Err(error) if error.is_terminal_identity_error() => {
                                break Err(error.into());
                            }
                            Err(error) => {
                                tracing::warn!(%error, "failed to refresh desired Agents for Runner restart");
                            }
                        }
                    }
                }
            }
        };

        background_shutdown.cancel();
        management_task.abort();
        heartbeat_task.abort();
        roster_task.abort();
        inventory_task.abort();
        let stop_result = stop_all(&mut runners).await;
        let _ = management_task.await;
        let _ = heartbeat_task.await;
        let _ = roster_task.await;
        let _ = inventory_task.await;
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
                if invalidation.is_none() {
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
    adapters: Vec<Arc<dyn EngineAdapter>>,
    interval: Duration,
    events: mpsc::Sender<DaemonEvent>,
    shutdown: CancellationToken,
) {
    loop {
        if !wait(interval, &shutdown).await {
            return;
        }
        let inventory = probe_engines(&adapters).await;
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

async fn probe_engines(adapters: &[Arc<dyn EngineAdapter>]) -> Vec<EngineProbe> {
    futures_util::future::join_all(adapters.iter().map(probe_engine)).await
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
    let engine_id = adapter.id().to_string();
    EngineProbe {
        readiness: EngineReadinessView {
            engine_id: engine_id.clone(),
            status,
        },
        observation: EngineObservation {
            engine_id,
            status,
            version: None,
            checked_at,
            last_error,
        },
    }
}

fn inventory_report(inventory: &[EngineProbe]) -> EngineInventoryReport {
    EngineInventoryReport {
        engines: inventory
            .iter()
            .map(|probe| probe.observation.clone())
            .collect(),
    }
}

fn actual_state(
    inventory: &[EngineProbe],
    mut runners: Vec<RunnerStatusView>,
) -> ComputerHeartbeatRequest {
    runners.sort_by(|left, right| left.agent_id.cmp(&right.agent_id));
    ComputerHeartbeatRequest {
        engine_readiness: inventory
            .iter()
            .map(|probe| probe.readiness.clone())
            .collect(),
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
    handle.stop_requested.cancel();
    handle.force_cancel.cancel();
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
    stop_all_with_grace(runners, RUNNER_SHUTDOWN_GRACE).await
}

async fn stop_all_with_grace(
    runners: &mut HashMap<String, RunnerHandle>,
    grace: Duration,
) -> Result<(), ComputerError> {
    let mut handles = runners
        .drain()
        .map(|(_, handle)| handle)
        .collect::<Vec<_>>();
    for handle in &handles {
        handle.stop_requested.cancel();
    }
    let deadline = tokio::time::Instant::now() + grace;
    while handles.iter().any(|handle| !handle.task.is_finished())
        && tokio::time::Instant::now() < deadline
    {
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    if handles.iter().any(|handle| !handle.task.is_finished()) {
        for handle in &handles {
            if !handle.task.is_finished() {
                handle.force_cancel.cancel();
            }
        }
    }
    let forced_deadline = tokio::time::Instant::now() + RUNNER_FORCE_REAP_TIMEOUT;
    while handles.iter().any(|handle| !handle.task.is_finished())
        && tokio::time::Instant::now() < forced_deadline
    {
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    let mut first_error = None;
    for handle in handles.drain(..) {
        if !handle.task.is_finished() {
            handle.task.abort();
            let _ = handle.task.await;
            if first_error.is_none() {
                first_error = Some(ComputerError::ShutdownTimeout);
            }
            continue;
        }
        match handle.task.await {
            Ok(Ok(())) => {}
            Ok(Err(error)) if first_error.is_none() => {
                first_error = Some(ComputerError::Runner(error));
            }
            Err(error) if first_error.is_none() => {
                first_error = Some(ComputerError::Task(error));
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
        inventory: &[EngineProbe],
        runners: &mut HashMap<String, RunnerHandle>,
        restart_plans: &mut HashMap<String, RestartPlan>,
    ) -> Result<Vec<RunnerStatusView>, ComputerError> {
        let desired = assignments
            .iter()
            .map(|assignment| (assignment.id.as_str(), assignment))
            .collect::<HashMap<_, _>>();
        restart_plans.retain(|id, _| desired.contains_key(id.as_str()));
        let mut failures = Vec::new();
        let removed = runners
            .keys()
            .filter(|id| {
                desired.get(id.as_str()).is_none_or(|assignment| {
                    **assignment != runners[*id].assignment
                        || !engine_runnable(inventory, &assignment.engine_id)
                })
            })
            .cloned()
            .collect::<Vec<_>>();
        for id in removed {
            if let Some(handle) = runners.remove(&id)
                && let Err(error) = stop_runner(handle).await
            {
                tracing::warn!(agent_id = id, %error, "Agent Runner required forced replacement");
            }
            restart_plans.remove(&id);
        }
        for assignment in assignments {
            if !runners.contains_key(&assignment.id) {
                if !engine_runnable(inventory, &assignment.engine_id) {
                    restart_plans.remove(&assignment.id);
                    failures.push(RunnerStatusView {
                        agent_id: assignment.id,
                        config_revision: assignment.config_revision,
                        state: RunnerState::Error,
                        last_error: Some(format!(
                            "Engine {} is not ready in this RuntimeSession",
                            assignment.engine_id
                        )),
                    });
                    continue;
                }
                let restart_failures = match restart_plans.get(&assignment.id) {
                    Some(plan) if plan.not_before > tokio::time::Instant::now() => {
                        failures.push(RunnerStatusView {
                            agent_id: assignment.id,
                            config_revision: assignment.config_revision,
                            state: RunnerState::Error,
                            last_error: Some("Agent Runner restart is scheduled".to_string()),
                        });
                        continue;
                    }
                    Some(plan) => plan.failures,
                    None => 0,
                };
                match self
                    .start(assignment.clone(), runners, restart_failures)
                    .await
                {
                    Ok(()) => {
                        restart_plans.remove(&assignment.id);
                    }
                    Err(error) if error.is_fenced() => return Err(error),
                    Err(error) => {
                        tracing::warn!(agent_id = assignment.id, %error, "Agent Runner could not start");
                        failures.push(RunnerStatusView {
                            agent_id: assignment.id.clone(),
                            config_revision: assignment.config_revision,
                            state: RunnerState::Error,
                            last_error: Some(bounded_error(error.to_string())),
                        });
                        restart_plans.insert(
                            assignment.id,
                            RestartPlan::after_failure(restart_failures.saturating_add(1)),
                        );
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
        restart_failures: u32,
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
                turn_timeout: None,
            })
            .await?;
        let stop_requested = CancellationToken::new();
        let task_stop_requested = stop_requested.clone();
        let force_cancel = CancellationToken::new();
        let task_force_cancel = force_cancel.clone();
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
        let task =
            tokio::spawn(async move { runner.run(task_stop_requested, task_force_cancel).await });
        runners.insert(
            id,
            RunnerHandle {
                assignment: handle_assignment,
                stop_requested,
                force_cancel,
                task,
                started_at: tokio::time::Instant::now(),
                restart_failures,
            },
        );
        Ok(())
    }
}

fn engine_runnable(inventory: &[EngineProbe], engine_id: &str) -> bool {
    inventory.iter().any(|probe| {
        probe.readiness.engine_id == engine_id && probe.readiness.status == EngineStatus::Ready
    })
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};

    fn assignment(id: &str) -> AgentAssignment {
        AgentAssignment {
            id: id.to_string(),
            display_name: id.to_string(),
            role: None,
            persona: "test".to_string(),
            engine_id: "fake".to_string(),
            main_model_id: "main".to_string(),
            triage_model_id: "triage".to_string(),
            config_revision: 1,
            agenda_enabled: false,
        }
    }

    #[test]
    fn engine_readiness_is_scoped_per_assignment() {
        let inventory = vec![
            EngineProbe {
                readiness: EngineReadinessView {
                    engine_id: "fake".to_string(),
                    status: EngineStatus::Ready,
                },
                observation: EngineObservation {
                    engine_id: "fake".to_string(),
                    status: EngineStatus::Ready,
                    version: None,
                    checked_at: 1,
                    last_error: None,
                },
            },
            EngineProbe {
                readiness: EngineReadinessView {
                    engine_id: "missing".to_string(),
                    status: EngineStatus::Missing,
                },
                observation: EngineObservation {
                    engine_id: "missing".to_string(),
                    status: EngineStatus::Missing,
                    version: None,
                    checked_at: 1,
                    last_error: None,
                },
            },
        ];

        assert!(engine_runnable(&inventory, "fake"));
        assert!(!engine_runnable(&inventory, "missing"));
        assert!(!engine_runnable(&inventory, "unregistered"));
        assert_eq!(
            actual_state(&inventory, Vec::new()).engine_readiness.len(),
            2
        );
    }

    #[tokio::test]
    async fn shutdown_gives_active_work_a_grace_period() {
        let stop_requested = CancellationToken::new();
        let force_cancel = CancellationToken::new();
        let observed_force = Arc::new(AtomicBool::new(false));
        let task_force = force_cancel.clone();
        let task_observed_force = observed_force.clone();
        let task = tokio::spawn(async move {
            tokio::select! {
                _ = tokio::time::sleep(Duration::from_millis(30)) => Ok(()),
                _ = task_force.cancelled() => {
                    task_observed_force.store(true, Ordering::SeqCst);
                    Ok(())
                }
            }
        });
        let mut runners = HashMap::from([(
            "agent-a".to_string(),
            RunnerHandle {
                assignment: assignment("agent-a"),
                stop_requested,
                force_cancel,
                task,
                started_at: tokio::time::Instant::now(),
                restart_failures: 0,
            },
        )]);

        stop_all_with_grace(&mut runners, Duration::from_millis(200))
            .await
            .expect("graceful shutdown");

        assert!(!observed_force.load(Ordering::SeqCst));
        assert!(runners.is_empty());
    }

    #[tokio::test]
    async fn shutdown_force_cancels_only_after_the_shared_deadline() {
        let started = tokio::time::Instant::now();
        let mut runners = HashMap::new();
        for id in ["agent-a", "agent-b"] {
            let stop_requested = CancellationToken::new();
            let force_cancel = CancellationToken::new();
            let task_force = force_cancel.clone();
            let task = tokio::spawn(async move {
                task_force.cancelled().await;
                Ok(())
            });
            runners.insert(
                id.to_string(),
                RunnerHandle {
                    assignment: assignment(id),
                    stop_requested,
                    force_cancel,
                    task,
                    started_at: tokio::time::Instant::now(),
                    restart_failures: 0,
                },
            );
        }

        stop_all_with_grace(&mut runners, Duration::from_millis(50))
            .await
            .expect("forced runners should still shut down cleanly");

        assert!(started.elapsed() >= Duration::from_millis(50));
        assert!(started.elapsed() < Duration::from_millis(250));
        assert!(runners.is_empty());
    }
}
