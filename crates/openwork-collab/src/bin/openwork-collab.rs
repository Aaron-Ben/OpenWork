use std::{env, error::Error, io};

use openwork_collab::{
    daemon::{DaemonConfig, IpcRequest, request, run},
    model::AgentInput,
};

type CliResult<T> = Result<T, Box<dyn Error + Send + Sync>>;

#[tokio::main]
async fn main() {
    if let Err(error) = run_cli().await {
        eprintln!("error: {error}");
        std::process::exit(1);
    }
}

async fn run_cli() -> CliResult<()> {
    let mut args = env::args().skip(1);
    let command = args.next().unwrap_or_else(|| "help".to_string());
    if command == "daemon" {
        return run(DaemonConfig::from_env()?).await.map_err(Into::into);
    }
    if matches!(command.as_str(), "help" | "--help" | "-h") {
        print_usage();
        return Ok(());
    }

    let ipc = match command.as_str() {
        "ping" => IpcRequest::Ping,
        "status" => IpcRequest::Status,
        "shutdown" => IpcRequest::Shutdown,
        "agent-create" => IpcRequest::CreateAgent {
            agent: agent_input(&mut args)?,
        },
        "agent-update" => IpcRequest::UpdateAgent {
            agent: agent_input(&mut args)?,
        },
        "agent-list" => IpcRequest::ListAgents,
        "room-create" => IpcRequest::CreateRoom {
            id: required(&mut args, "room id")?,
            title: required(&mut args, "room title")?,
        },
        "room-add" => IpcRequest::AddMember {
            room_id: required(&mut args, "room id")?,
            participant_id: required(&mut args, "participant id")?,
        },
        "send" => IpcRequest::SendMessage {
            room_id: required(&mut args, "room id")?,
            author_id: required(&mut args, "author id")?,
            body: required(&mut args, "message body")?,
        },
        "messages" => IpcRequest::Messages {
            room_id: required(&mut args, "room id")?,
        },
        "permissions" => IpcRequest::Permissions,
        "credential-check" => IpcRequest::CredentialCheck {
            provider_id: required(&mut args, "provider id")?,
        },
        unknown => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("unknown command {unknown:?}; run with --help"),
            )
            .into());
        }
    };
    if args.next().is_some() {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "too many arguments").into());
    }
    let config = DaemonConfig::from_env()?;
    let response = request(&config.socket_path(), &ipc).await?;
    println!(
        "{}",
        serde_json::to_string_pretty(&response.data.unwrap_or(serde_json::Value::Null))?
    );
    if !response.ok {
        return Err(io::Error::other(
            response
                .error
                .unwrap_or_else(|| "daemon request failed".to_string()),
        )
        .into());
    }
    Ok(())
}

fn agent_input(args: &mut impl Iterator<Item = String>) -> CliResult<AgentInput> {
    Ok(AgentInput {
        id: required(args, "agent id")?,
        display_name: required(args, "display name")?,
        provider_id: required(args, "provider id")?,
        model_id: required(args, "model id")?,
        system_prompt: required(args, "system prompt")?,
        role: None,
        bio: None,
        enabled: true,
    })
}

fn required(args: &mut impl Iterator<Item = String>, name: &'static str) -> CliResult<String> {
    args.next().ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidInput, format!("missing {name}")).into()
    })
}

fn print_usage() {
    println!(
        "OpenWork collaboration P1\n\n\
         Usage:\n\
           openwork-collab daemon\n\
           openwork-collab status | shutdown | agent-list | permissions\n\
           openwork-collab agent-create <id> <display-name> <provider-id> <model-id> <system-prompt>\n\
           openwork-collab agent-update <id> <display-name> <provider-id> <model-id> <system-prompt>\n\
           openwork-collab room-create <id> <title>\n\
           openwork-collab room-add <room-id> <participant-id>\n\
           openwork-collab send <room-id> <author-id> <body>\n\
           openwork-collab messages <room-id>\n\
           openwork-collab credential-check <provider-id>\n\n\
         Environment:\n\
           DATABASE_URL            PostgreSQL URL\n\
           OPENWORK_COLLAB_HOME     daemon state root (default ~/.openwork/collab)\n\
           OPENCODE_BIN             alternate opencode executable\n\
           OPENCODE_SERVER_PASSWORD optional OpenCode basic-auth password"
    );
}
