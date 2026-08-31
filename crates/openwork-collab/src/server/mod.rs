mod agenda;
mod agents;
mod auth;
mod board;
mod cli;
mod computers;
pub mod control;
mod coordination;
mod db;
mod messages;
mod migration;
mod redis;
mod rooms;
mod runs;
mod runtime;
mod scheduler;
mod triage;

use std::{net::SocketAddr, path::PathBuf, time::Duration};

use sqlx::PgPool;
use thiserror::Error;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use agenda::Agenda;
use agents::Agents;
use board::Board;
use cli::CliDispatcher;
use computers::Computers;
use coordination::Coordination;
use messages::Messages;
use rooms::Rooms;
use runs::Runs;
use scheduler::Scheduler;
use triage::InboxTriage;

#[derive(Clone, Debug)]
pub struct ServerOptions {
    pub database_url: String,
    pub redis_url: String,
    pub state_root: PathBuf,
    pub control_socket: PathBuf,
    pub runtime_bind: SocketAddr,
    pub computer_lease: Duration,
    pub offline_sweep_interval: Duration,
}

pub struct CollaborationServer;

pub struct ServerHandle {
    shutdown: CancellationToken,
    control_socket: PathBuf,
    pool: PgPool,
    tasks: Vec<JoinHandle<()>>,
    runtime_addr: SocketAddr,
}

impl CollaborationServer {
    pub async fn start(
        options: ServerOptions,
        shutdown: CancellationToken,
    ) -> Result<ServerHandle, ServerError> {
        if !options.runtime_bind.ip().is_loopback() {
            return Err(ServerError::NonLoopbackBind(options.runtime_bind));
        }
        tokio::fs::create_dir_all(&options.state_root).await?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            tokio::fs::set_permissions(&options.state_root, std::fs::Permissions::from_mode(0o700))
                .await?;
        }

        let pool = db::connect(&options.database_url).await?;
        let agents = Agents::new(pool.clone());
        let board = Board::new(pool.clone());
        let computers = Computers::new(pool.clone());
        let rooms = Rooms::new(pool.clone());
        let messages = Messages::new(pool.clone());
        let runs = Runs::new(pool.clone());
        let triage = InboxTriage::new(pool.clone());
        let signing_key = auth::load_or_create_signing_key(&options.state_root).await?;
        let (coordination, redis_task) =
            redis::RedisCoordination::start(&options.redis_url, shutdown.clone()).await?;
        let redis_coordination = coordination;
        let coordination = Coordination::new(redis_coordination.clone());
        let scheduler = Scheduler::new(
            rooms.clone(),
            messages.clone(),
            redis_coordination,
            coordination.clone(),
        );
        let cli = CliDispatcher::new(pool.clone(), coordination.clone());
        let agenda = Agenda::new(pool.clone(), coordination.clone(), signing_key.clone());
        let scheduler_task = scheduler.start(shutdown.clone());
        let sweep_computers = computers.clone();
        let sweep_shutdown = shutdown.clone();
        let computer_lease = options.computer_lease;
        let offline_sweep_interval = options.offline_sweep_interval;
        let offline_sweep_task = tokio::spawn(async move {
            let mut interval = tokio::time::interval(offline_sweep_interval);
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                tokio::select! {
                    _ = sweep_shutdown.cancelled() => return,
                    _ = interval.tick() => {
                        if let Err(error) = sweep_computers.sweep_offline(computer_lease).await {
                            tracing::warn!(%error, "Local Computer offline sweep failed");
                        }
                    }
                }
            }
        });

        let control = control::bind(&options.control_socket).await?;
        let runtime = tokio::net::TcpListener::bind(options.runtime_bind).await?;
        let runtime_addr = runtime.local_addr()?;
        let runtime_base_url = format!("http://{runtime_addr}");

        let control_shutdown = shutdown.clone();
        let control_state = control::ControlState::new(
            agents.clone(),
            board,
            computers.clone(),
            rooms,
            messages.clone(),
            runs.clone(),
            scheduler.clone(),
        );
        let control_task = tokio::spawn(async move {
            control::serve(control, control_state, runtime_base_url, control_shutdown).await;
        });

        let runtime_shutdown = shutdown.clone();
        let runtime_task = tokio::spawn(async move {
            let app = runtime::router(runtime::RuntimeState {
                agents,
                computers,
                messages,
                runs,
                signing_key,
                scheduler,
                coordination,
                triage,
                agenda,
                cli,
            });
            let _ = axum::serve(runtime, app)
                .with_graceful_shutdown(runtime_shutdown.cancelled_owned())
                .await;
        });

        Ok(ServerHandle {
            shutdown,
            control_socket: options.control_socket,
            pool,
            tasks: vec![
                redis_task,
                scheduler_task,
                offline_sweep_task,
                control_task,
                runtime_task,
            ],
            runtime_addr,
        })
    }
}

impl ServerHandle {
    pub fn runtime_addr(&self) -> SocketAddr {
        self.runtime_addr
    }

    pub async fn shutdown(self) -> Result<(), ServerError> {
        self.shutdown.cancel();
        for task in self.tasks {
            task.await?;
        }
        self.pool.close().await;
        match tokio::fs::remove_file(&self.control_socket).await {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
        Ok(())
    }
}

#[derive(Debug, Error)]
pub enum ServerError {
    #[error("collaboration Runtime must bind to loopback, got {0}")]
    NonLoopbackBind(SocketAddr),
    #[error("database error: {0}")]
    Database(#[from] sqlx::Error),
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Redis configuration error: {0}")]
    Redis(#[from] ::redis::RedisError),
    #[error("server task failed: {0}")]
    Task(#[from] tokio::task::JoinError),
}
