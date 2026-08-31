mod agenda;
mod agent_commands;
mod agents;
mod auth;
mod board;
mod climate;
mod command_requests;
mod coordination;
mod db;
mod desktop_commands;
mod inventory;
mod messages;
mod migration;
mod redis;
mod rooms;
mod runs;
mod runtime_session;
mod scheduler;
mod transport;
mod triage;

use std::net::SocketAddr;

use sqlx::PgPool;
use thiserror::Error;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use agenda::Agenda;
use agent_commands::AgentCommands;
use agents::Agents;
use board::Board;
use coordination::Coordination;
use desktop_commands::DesktopCommands;
use inventory::EngineInventory;
use messages::Messages;
use rooms::Rooms;
use runs::Runs;
use scheduler::Scheduler;
use transport::TransportState;
use triage::InboxTriage;

pub use runtime_session::{RuntimeCredentials, RuntimeSessionError};

pub struct ServerOptions {
    pub database_url: String,
    pub redis_url: String,
    pub runtime_bind: SocketAddr,
    pub credentials: RuntimeCredentials,
}

pub struct CollaborationServer;

pub struct ServerHandle {
    shutdown: CancellationToken,
    failed: CancellationToken,
    pool: PgPool,
    runtime_session_id: String,
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
        if options.runtime_bind.port() != 0 {
            return Err(ServerError::FixedPort(options.runtime_bind));
        }
        let session = runtime_session::RuntimeSession::new(options.credentials)?;
        let pool = db::connect(&options.database_url).await?;
        let agents = Agents::new(pool.clone());
        let board = Board::new(pool.clone());
        let rooms = Rooms::new(pool.clone());
        let messages = Messages::new(pool.clone());
        let runs = Runs::new(pool.clone());
        runs.interrupt_stale(session.id()).await?;
        let triage = InboxTriage::new(pool.clone());
        let inventory = EngineInventory::new(pool.clone());
        let (redis_coordination, redis_task) =
            redis::RedisCoordination::start(&options.redis_url, shutdown.clone()).await?;
        let coordination = Coordination::new(redis_coordination.clone());
        let scheduler = Scheduler::new(
            rooms.clone(),
            messages.clone(),
            redis_coordination,
            coordination.clone(),
        );
        let scheduler_task = scheduler.start(shutdown.clone());
        let agenda = Agenda::new(
            pool.clone(),
            coordination.clone(),
            session.signing_key().clone(),
        );
        let agent_commands = AgentCommands::new(pool.clone(), coordination.clone());
        let desktop_commands = DesktopCommands::new(
            pool.clone(),
            agents.clone(),
            board,
            inventory.clone(),
            messages.clone(),
            rooms,
            runs.clone(),
            scheduler.clone(),
            session.clone(),
        );

        let listener = tokio::net::TcpListener::bind(options.runtime_bind).await?;
        let runtime_addr = listener.local_addr()?;
        let runtime_shutdown = shutdown.clone();
        let app = transport::router(TransportState {
            agents,
            messages,
            runs,
            scheduler,
            coordination,
            triage,
            agenda,
            agent_commands,
            desktop_commands,
            inventory,
            session: session.clone(),
        });
        let runtime_task = tokio::spawn(async move {
            if let Err(error) = axum::serve(listener, app)
                .with_graceful_shutdown(runtime_shutdown.cancelled_owned())
                .await
            {
                tracing::error!(%error, "Collaboration Server transport stopped");
            }
        });
        session.publish_runtime_ready();

        let failed = CancellationToken::new();
        let tasks = vec![
            monitor_task(
                "Redis subscriber",
                redis_task,
                shutdown.clone(),
                failed.clone(),
            ),
            monitor_task(
                "message scheduler",
                scheduler_task,
                shutdown.clone(),
                failed.clone(),
            ),
            monitor_task(
                "HTTP transport",
                runtime_task,
                shutdown.clone(),
                failed.clone(),
            ),
        ];

        Ok(ServerHandle {
            shutdown,
            failed,
            pool,
            runtime_session_id: session.id().to_string(),
            tasks,
            runtime_addr,
        })
    }
}

impl ServerHandle {
    pub fn runtime_addr(&self) -> SocketAddr {
        self.runtime_addr
    }

    pub async fn failed(&self) {
        self.failed.cancelled().await;
    }

    pub async fn shutdown(self) -> Result<(), ServerError> {
        self.shutdown.cancel();
        for task in self.tasks {
            task.await?;
        }
        Runs::new(self.pool.clone())
            .interrupt_session(&self.runtime_session_id)
            .await?;
        self.pool.close().await;
        Ok(())
    }
}

fn monitor_task(
    name: &'static str,
    task: JoinHandle<()>,
    shutdown: CancellationToken,
    failed: CancellationToken,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let outcome = task.await;
        if !shutdown.is_cancelled() {
            match outcome {
                Ok(()) => tracing::error!(task = name, "Collaboration Server task stopped"),
                Err(error) => {
                    tracing::error!(task = name, %error, "Collaboration Server task failed")
                }
            }
            failed.cancel();
        }
    })
}

#[derive(Debug, Error)]
pub enum ServerError {
    #[error("Collaboration Server must bind to loopback, got {0}")]
    NonLoopbackBind(SocketAddr),
    #[error("Collaboration Server must use an OS-assigned port, got {0}")]
    FixedPort(SocketAddr),
    #[error("database error: {0}")]
    Database(#[from] sqlx::Error),
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Redis configuration error: {0}")]
    Redis(#[from] ::redis::RedisError),
    #[error("RuntimeSession error: {0}")]
    RuntimeSession(#[from] RuntimeSessionError),
    #[error("Collaboration Server background task stopped unexpectedly")]
    BackgroundStopped,
    #[error("Server task failed: {0}")]
    Task(#[from] tokio::task::JoinError),
}
