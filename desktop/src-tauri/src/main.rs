use std::{net::SocketAddr, path::PathBuf, sync::Arc, time::Duration};

use openwork_collab::{
    computer::{
        daemon::{ComputerDaemon, ComputerOptions},
        opencode::OpenCodeAdapter,
    },
    server::{CollaborationServer, ServerOptions},
};
use serde::Deserialize;
use tokio_util::sync::CancellationToken;

fn main() {
    let _ = dotenvy::dotenv();
    let invoked_name = std::env::args_os()
        .next()
        .and_then(|path| PathBuf::from(path).file_name().map(|name| name.to_owned()));
    if invoked_name.as_deref() == Some(std::ffi::OsStr::new("openwork")) {
        std::process::exit(openwork_collab::computer::shim::main());
    }
    let role = std::env::args().nth(1);
    if role.as_deref() == Some("--openwork-collab-server") {
        run_role(run_server());
        return;
    }
    if role.as_deref() == Some("--openwork-collab-computer") {
        run_role(run_computer());
        return;
    }
    openwork_desktop_lib::run()
}

fn run_role(future: impl std::future::Future<Output = Result<(), Box<dyn std::error::Error>>>) {
    let _ = dotenvy::dotenv();
    let runtime = tokio::runtime::Runtime::new().expect("create local collaboration runtime");
    if let Err(error) = runtime.block_on(future) {
        eprintln!("local collaboration process failed: {error}");
        std::process::exit(1);
    }
}

async fn run_server() -> Result<(), Box<dyn std::error::Error>> {
    let root = state_root()?;
    let shutdown = CancellationToken::new();
    let runtime_bind = std::env::var("OPENWORK_COLLAB_RUNTIME_BIND")
        .unwrap_or_else(|_| "127.0.0.1:17843".to_string())
        .parse::<SocketAddr>()?;
    let server = CollaborationServer::start(
        ServerOptions {
            database_url: std::env::var("DATABASE_URL")?,
            redis_url: std::env::var("REDIS_URL")?,
            state_root: root.join("server"),
            control_socket: root.join("server/control.sock"),
            runtime_bind,
            computer_lease: Duration::from_secs(90),
            offline_sweep_interval: Duration::from_secs(15),
        },
        shutdown.clone(),
    )
    .await?;
    tokio::select! {
        _ = shutdown.cancelled() => {}
        signal = shutdown_signal() => signal?,
    }
    shutdown.cancel();
    server.shutdown().await?;
    Ok(())
}

async fn run_computer() -> Result<(), Box<dyn std::error::Error>> {
    let root = state_root()?;
    let identity: ComputerIdentity =
        serde_json::from_slice(&tokio::fs::read(root.join("computer/computer.json")).await?)?;
    if identity.protocol_version != openwork_collab::protocol::COLLAB_PROTOCOL_VERSION
        || identity.computer_id != "local"
    {
        return Err("Local Computer identity is incompatible".into());
    }
    let opencode = std::env::var("OPENCODE_BIN").unwrap_or_else(|_| "opencode".to_string());
    let shutdown = CancellationToken::new();
    let daemon = ComputerDaemon::new(
        ComputerOptions {
            state_root: root.join("computer"),
            runtime_base_url: identity.runtime_base_url,
            device_token: identity.device_token,
            shim_executable: std::env::current_exe()?,
            supervised: std::env::var("OPENWORK_COLLAB_SUPERVISED").as_deref() == Ok("1"),
            poll_interval: Duration::from_secs(20),
            roster_interval: Duration::from_secs(60),
            heartbeat_interval: Duration::from_secs(30),
            engine_rescan_interval: Duration::from_secs(5 * 60),
        },
        Arc::new(OpenCodeAdapter::with_executable(opencode)),
    );
    let run = daemon.run(shutdown.clone());
    tokio::pin!(run);
    tokio::select! {
        result = &mut run => result?,
        signal = shutdown_signal() => {
            signal?;
            shutdown.cancel();
            tokio::time::timeout(Duration::from_secs(20), &mut run)
                .await
                .map_err(|_| "Local Computer did not stop within 20 seconds")??;
        }
    }
    Ok(())
}

async fn shutdown_signal() -> Result<(), std::io::Error> {
    let mut terminate = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
    tokio::select! {
        result = tokio::signal::ctrl_c() => result,
        _ = terminate.recv() => Ok(()),
    }
}

fn state_root() -> Result<PathBuf, Box<dyn std::error::Error>> {
    std::env::var_os("OPENWORK_COLLAB_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".openwork")))
        .ok_or_else(|| "HOME or OPENWORK_COLLAB_HOME must be set".into())
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
struct ComputerIdentity {
    protocol_version: u32,
    computer_id: String,
    runtime_base_url: String,
    device_token: String,
}
