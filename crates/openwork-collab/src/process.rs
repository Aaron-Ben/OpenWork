use std::{net::SocketAddr, path::PathBuf, time::Duration};

use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio_util::sync::CancellationToken;

use crate::{
    computer::{
        daemon::{ComputerDaemon, ComputerOptions},
        engine::EngineRegistry,
        opencode::OpenCodeAdapter,
    },
    protocol::{
        ComputerProcessBootstrap, ComputerProcessReady, ServerProcessBootstrap, ServerProcessReady,
    },
    server::{CollaborationServer, RuntimeCredentials, ServerOptions},
};

pub async fn run_server_process() -> Result<(), ProcessError> {
    let bootstrap = read_bootstrap::<ServerProcessBootstrap>().await?;
    let shutdown = CancellationToken::new();
    let server = CollaborationServer::start(
        ServerOptions {
            database_url: std::env::var("DATABASE_URL")?,
            redis_url: std::env::var("REDIS_URL")?,
            runtime_bind: "127.0.0.1:0"
                .parse::<SocketAddr>()
                .expect("static loopback address is valid"),
            credentials: RuntimeCredentials {
                runtime_session_id: bootstrap.runtime_session_id.clone(),
                desktop_secret: bootstrap.desktop_secret,
                computer_secret: bootstrap.computer_secret,
            },
        },
        shutdown.clone(),
    )
    .await?;
    write_ready(&ServerProcessReady {
        runtime_session_id: bootstrap.runtime_session_id,
        base_url: format!("http://{}", server.runtime_addr()),
    })
    .await?;
    let background_failed = tokio::select! {
        signal = shutdown_signal() => {
            signal?;
            false
        }
        _ = server.failed() => true,
    };
    shutdown.cancel();
    server.shutdown().await?;
    if background_failed {
        return Err(crate::server::ServerError::BackgroundStopped.into());
    }
    Ok(())
}

pub async fn run_computer_process() -> Result<(), ProcessError> {
    let bootstrap = read_bootstrap::<ComputerProcessBootstrap>().await?;
    let shutdown = CancellationToken::new();
    let daemon = ComputerDaemon::new(
        ComputerOptions {
            openwork_root: PathBuf::from(&bootstrap.openwork_root),
            runtime_session_id: bootstrap.runtime_session_id.clone(),
            runtime_base_url: bootstrap.base_url,
            computer_secret: bootstrap.computer_secret,
            shim_executable: PathBuf::from(bootstrap.shim_executable),
            poll_interval: Duration::from_secs(20),
            roster_interval: Duration::from_secs(60),
            heartbeat_interval: Duration::from_secs(30),
            engine_rescan_interval: Duration::from_secs(5 * 60),
        },
        EngineRegistry::single(OpenCodeAdapter::with_executable(
            bootstrap.engine_executable,
        )),
    );
    write_ready(&ComputerProcessReady {
        runtime_session_id: bootstrap.runtime_session_id,
    })
    .await?;
    let run = daemon.run(shutdown.clone());
    tokio::pin!(run);
    tokio::select! {
        result = &mut run => result?,
        signal = shutdown_signal() => {
            signal?;
            shutdown.cancel();
            tokio::time::timeout(Duration::from_secs(17), &mut run)
                .await
                .map_err(|_| ProcessError::ComputerShutdownTimeout)??;
        }
    }
    Ok(())
}

async fn read_bootstrap<T: serde::de::DeserializeOwned>() -> Result<T, ProcessError> {
    let mut line = String::new();
    let mut reader = BufReader::new(tokio::io::stdin()).take(64 * 1024 + 1);
    let bytes = reader.read_line(&mut line).await?;
    if bytes == 0 || bytes > 64 * 1024 {
        return Err(ProcessError::InvalidBootstrap);
    }
    serde_json::from_str(&line).map_err(ProcessError::BootstrapJson)
}

async fn write_ready(value: &impl serde::Serialize) -> Result<(), ProcessError> {
    let mut stdout = tokio::io::stdout();
    let mut bytes = serde_json::to_vec(value)?;
    bytes.push(b'\n');
    stdout.write_all(&bytes).await?;
    stdout.flush().await?;
    Ok(())
}

async fn shutdown_signal() -> Result<(), std::io::Error> {
    let mut terminate = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
    tokio::select! {
        result = tokio::signal::ctrl_c() => result,
        _ = terminate.recv() => Ok(()),
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ProcessError {
    #[error("process bootstrap is missing or too large")]
    InvalidBootstrap,
    #[error("process bootstrap is invalid: {0}")]
    BootstrapJson(serde_json::Error),
    #[error("process response could not be encoded: {0}")]
    ResponseJson(#[from] serde_json::Error),
    #[error("process environment is incomplete: {0}")]
    Environment(#[from] std::env::VarError),
    #[error("process I/O failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("Collaboration Server failed: {0}")]
    Server(#[from] crate::server::ServerError),
    #[error("Local Computer failed: {0}")]
    Computer(#[from] crate::computer::daemon::ComputerError),
    #[error("Local Computer did not stop within the bounded shutdown window")]
    ComputerShutdownTimeout,
}
