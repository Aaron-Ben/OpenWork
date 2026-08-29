use std::{collections::HashMap, path::PathBuf, sync::Arc, time::Duration};

use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use crate::protocol::{
    AgentAssignment, COLLAB_PROTOCOL_VERSION, EngineProbeView, HeartbeatRequest,
};

use super::{
    client::{DeviceClient, RuntimeClientError},
    engine::{EngineAdapter, EngineProbeStatus},
    home::{HomeError, HomeManager},
    runner::{AgentRunner, RunnerError, RunnerIdentity},
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
}

pub struct ComputerDaemon<A: EngineAdapter + 'static> {
    options: ComputerOptions,
    adapter: Arc<A>,
}

struct RunnerHandle {
    config_version: i64,
    shutdown: CancellationToken,
    task: JoinHandle<Result<(), RunnerError>>,
}

impl<A: EngineAdapter + 'static> ComputerDaemon<A> {
    pub fn new(options: ComputerOptions, adapter: Arc<A>) -> Self {
        Self { options, adapter }
    }

    pub async fn run(self, shutdown: CancellationToken) -> Result<(), ComputerError> {
        let client = DeviceClient::new(
            self.options.runtime_base_url.clone(),
            self.options.device_token.clone(),
        );
        let probe = self.adapter.probe().await?;
        let generation = client.start().await?;
        client
            .heartbeat(&HeartbeatRequest {
                protocol_version: COLLAB_PROTOCOL_VERSION,
                generation,
                daemon_version: env!("CARGO_PKG_VERSION").to_string(),
                supervised: self.options.supervised,
                engine: EngineProbeView {
                    engine_id: "opencode".to_string(),
                    status: probe.status.into(),
                    version: probe.version,
                },
            })
            .await?;
        let home_manager = HomeManager::new(
            self.options.state_root,
            self.options.shim_executable,
            self.options.runtime_base_url,
        );
        let mut runners = HashMap::<String, RunnerHandle>::new();
        if probe.status == EngineProbeStatus::Ready {
            reconcile(
                &client,
                generation,
                &home_manager,
                &self.adapter,
                self.options.poll_interval,
                &mut runners,
            )
            .await?;
        }

        let mut roster_tick = tokio::time::interval(self.options.roster_interval);
        roster_tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        roster_tick.tick().await;
        loop {
            tokio::select! {
                _ = shutdown.cancelled() => break,
                _ = roster_tick.tick() => {
                    let probe = self.adapter.probe().await?;
                    client.heartbeat(&HeartbeatRequest {
                        protocol_version: COLLAB_PROTOCOL_VERSION,
                        generation,
                        daemon_version: env!("CARGO_PKG_VERSION").to_string(),
                        supervised: self.options.supervised,
                        engine: EngineProbeView {
                            engine_id: "opencode".to_string(),
                            status: probe.status.into(),
                            version: probe.version,
                        },
                    }).await?;
                    if probe.status == EngineProbeStatus::Ready {
                        reconcile(
                            &client,
                            generation,
                            &home_manager,
                            &self.adapter,
                            self.options.poll_interval,
                            &mut runners,
                        ).await?;
                    } else {
                        stop_all(&mut runners).await?;
                    }
                    for handle in runners.values() {
                        if handle.task.is_finished() {
                            return Err(ComputerError::RunnerStopped);
                        }
                    }
                }
            }
        }
        stop_all(&mut runners).await?;
        Ok(())
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

async fn reconcile<A: EngineAdapter + 'static>(
    client: &DeviceClient,
    generation: i64,
    home_manager: &HomeManager,
    adapter: &Arc<A>,
    poll_interval: Duration,
    runners: &mut HashMap<String, RunnerHandle>,
) -> Result<(), ComputerError> {
    let assignments = client.roster(generation).await?;
    let versions: HashMap<&str, i64> = assignments
        .iter()
        .map(|assignment| (assignment.id.as_str(), assignment.config_version))
        .collect();
    let removed: Vec<String> = runners
        .keys()
        .filter(|id| versions.get(id.as_str()).copied() != Some(runners[*id].config_version))
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
            start_runner(
                client,
                generation,
                home_manager,
                adapter,
                poll_interval,
                assignment,
                runners,
            )
            .await?;
        }
    }
    Ok(())
}

async fn start_runner<A: EngineAdapter + 'static>(
    client: &DeviceClient,
    generation: i64,
    home_manager: &HomeManager,
    adapter: &Arc<A>,
    poll_interval: Duration,
    assignment: AgentAssignment,
    runners: &mut HashMap<String, RunnerHandle>,
) -> Result<(), ComputerError> {
    let runtime_token = client.mint_agent_token(&assignment.id, generation).await?;
    let home = home_manager
        .materialize(&assignment, &runtime_token.token)
        .await?;
    let runner_shutdown = CancellationToken::new();
    let task_shutdown = runner_shutdown.clone();
    let id = assignment.id.clone();
    let config_version = assignment.config_version;
    let runner = AgentRunner::new(
        assignment,
        RunnerIdentity {
            device: client.clone(),
            generation,
            token_expires_at: runtime_token.expires_at,
        },
        client.agent(runtime_token.token),
        adapter.clone(),
        home,
        poll_interval,
    );
    let task = tokio::spawn(async move { runner.run(task_shutdown).await });
    runners.insert(
        id,
        RunnerHandle {
            config_version,
            shutdown: runner_shutdown,
            task,
        },
    );
    Ok(())
}

impl From<EngineProbeStatus> for crate::protocol::EngineStatus {
    fn from(status: EngineProbeStatus) -> Self {
        match status {
            EngineProbeStatus::Ready => Self::Ready,
            EngineProbeStatus::Missing => Self::Missing,
            EngineProbeStatus::Unauthenticated => Self::Unauthenticated,
            EngineProbeStatus::Broken => Self::Broken,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ComputerError {
    #[error(transparent)]
    Runtime(#[from] RuntimeClientError),
    #[error(transparent)]
    Engine(#[from] super::engine::EngineError),
    #[error(transparent)]
    Home(#[from] HomeError),
    #[error(transparent)]
    Runner(#[from] RunnerError),
    #[error("Agent runner task failed: {0}")]
    Task(#[from] tokio::task::JoinError),
    #[error("Agent runner stopped unexpectedly")]
    RunnerStopped,
}
