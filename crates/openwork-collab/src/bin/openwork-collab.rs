use std::{error::Error, net::SocketAddr, path::PathBuf, sync::Arc, time::Duration};

use openwork_collab::{
    computer::{
        daemon::{ComputerDaemon, ComputerOptions},
        opencode::OpenCodeAdapter,
    },
    protocol::{ControlRequest, ControlResponse},
    server::{CollaborationServer, ServerOptions, control::request},
};
use serde::Deserialize;
use tokio_util::sync::CancellationToken;

type CliResult<T> = Result<T, Box<dyn Error + Send + Sync>>;

#[tokio::main]
async fn main() {
    if let Err(error) = run().await {
        eprintln!("error: {error}");
        std::process::exit(1);
    }
}

async fn run() -> CliResult<()> {
    let mut arguments = std::env::args().skip(1);
    match arguments.next().as_deref() {
        Some("server") => run_server().await,
        Some("computer") => run_computer().await,
        Some("ensure-local-computer") => {
            print_response(control(ControlRequest::EnsureLocalComputer).await?)
        }
        Some("agent-create") => {
            let id = required(&mut arguments, "agent id")?;
            let display_name = required(&mut arguments, "display name")?;
            let model = required(&mut arguments, "model")?;
            let system_prompt = required(&mut arguments, "system prompt")?;
            print_response(
                control(ControlRequest::CreateAgent {
                    id,
                    display_name,
                    system_prompt,
                    model,
                })
                .await?,
            )
        }
        Some("dm-create") => {
            let agent_id = required(&mut arguments, "agent id")?;
            print_response(control(ControlRequest::CreateDirectRoom { agent_id }).await?)
        }
        Some("send") => {
            let room_id = required(&mut arguments, "room id")?;
            let body = required(&mut arguments, "message body")?;
            print_response(control(ControlRequest::SendMessage { room_id, body }).await?)
        }
        Some("messages") => {
            let room_id = required(&mut arguments, "room id")?;
            print_response(control(ControlRequest::ListMessages { room_id }).await?)
        }
        Some("help" | "--help" | "-h") | None => {
            print_usage();
            Ok(())
        }
        Some(command) => Err(format!("unknown command {command:?}; run with --help").into()),
    }
}

async fn run_server() -> CliResult<()> {
    let state_root = state_root().join("server");
    let runtime_bind: SocketAddr = std::env::var("OPENWORK_COLLAB_RUNTIME_BIND")
        .unwrap_or_else(|_| "127.0.0.1:17843".to_string())
        .parse()?;
    let shutdown = CancellationToken::new();
    let server = CollaborationServer::start(
        ServerOptions {
            database_url: std::env::var("DATABASE_URL")?,
            redis_url: std::env::var("REDIS_URL")?,
            state_root,
            control_socket: control_socket(),
            runtime_bind,
            computer_lease: Duration::from_secs(90),
            offline_sweep_interval: Duration::from_secs(15),
        },
        shutdown.clone(),
    )
    .await?;
    println!(
        "Collaboration Runtime listening on {}",
        server.runtime_addr()
    );
    tokio::signal::ctrl_c().await?;
    shutdown.cancel();
    server.shutdown().await?;
    Ok(())
}

async fn run_computer() -> CliResult<()> {
    let identity: ComputerIdentity = serde_json::from_slice(
        &tokio::fs::read(state_root().join("computer/computer.json")).await?,
    )?;
    if identity.protocol_version != openwork_collab::protocol::COLLAB_PROTOCOL_VERSION
        || identity.computer_id != "local"
    {
        return Err("computer identity has an incompatible protocol or id".into());
    }
    let shim_executable = std::env::current_exe()?.with_file_name("openwork");
    let opencode = std::env::var("OPENCODE_BIN").unwrap_or_else(|_| "opencode".to_string());
    let shutdown = CancellationToken::new();
    let daemon = ComputerDaemon::new(
        ComputerOptions {
            state_root: state_root().join("computer"),
            runtime_base_url: identity.runtime_base_url,
            device_token: identity.device_token,
            shim_executable,
            supervised: false,
            poll_interval: Duration::from_secs(20),
            roster_interval: Duration::from_secs(60),
            heartbeat_interval: Duration::from_secs(30),
            probe_interval: Duration::from_secs(5 * 60),
        },
        Arc::new(OpenCodeAdapter::with_executable(opencode)),
    );
    tokio::select! {
        result = daemon.run(shutdown.clone()) => result.map_err(Into::into),
        result = tokio::signal::ctrl_c() => {
            result?;
            shutdown.cancel();
            Ok(())
        }
    }
}

async fn control(request_value: ControlRequest) -> CliResult<ControlResponse> {
    Ok(request(&control_socket(), &request_value).await?)
}

fn print_response(response: ControlResponse) -> CliResult<()> {
    match response {
        ControlResponse::Error { message } => Err(message.into()),
        response => {
            println!("{}", serde_json::to_string_pretty(&response)?);
            Ok(())
        }
    }
}

fn state_root() -> PathBuf {
    std::env::var_os("OPENWORK_COLLAB_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".openwork")))
        .expect("HOME or OPENWORK_COLLAB_HOME must be set")
}

fn control_socket() -> PathBuf {
    state_root().join("server/control.sock")
}

fn required(arguments: &mut impl Iterator<Item = String>, name: &str) -> CliResult<String> {
    arguments
        .next()
        .ok_or_else(|| format!("missing {name}").into())
}

fn print_usage() {
    println!(
        "OpenWork local collaboration (macOS, OpenCode only)\n\n\
         Usage:\n\
           openwork-collab server\n\
           openwork-collab computer\n\
           openwork-collab ensure-local-computer\n\
           openwork-collab agent-create <id> <display-name> <model> <system-prompt>\n\
           openwork-collab dm-create <agent-id>\n\
           openwork-collab send <room-id> <body>\n\
           openwork-collab messages <room-id>"
    );
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
struct ComputerIdentity {
    protocol_version: u32,
    computer_id: String,
    runtime_base_url: String,
    device_token: String,
}
