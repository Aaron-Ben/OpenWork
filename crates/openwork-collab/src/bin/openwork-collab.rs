use std::{env, error::Error, io};

use openwork_collab::{
    daemon::{DaemonConfig, IpcRequest, request, run},
    model::{AgentInput, CardInput},
    opencode::PermissionReply,
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
        "dm-create" => IpcRequest::CreateDirectRoom {
            first_participant: required(&mut args, "first participant id")?,
            second_participant: required(&mut args, "second participant id")?,
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
        "permission-reply" => IpcRequest::ReplyPermission {
            id: required(&mut args, "permission id")?,
            reply: permission_reply(&required(&mut args, "reply")?)?,
            message: args.next(),
        },
        "permission-abort" => IpcRequest::AbortPermission {
            id: required(&mut args, "permission id")?,
        },
        "credential-check" => IpcRequest::CredentialCheck {
            provider_id: required(&mut args, "provider id")?,
        },
        "triage-config" => IpcRequest::ConfigureTriage {
            provider_id: required(&mut args, "provider id")?,
            model_id: required(&mut args, "model id")?,
        },
        "triage-list" => IpcRequest::ListTriages {
            room_id: args.next(),
        },
        "logs" => IpcRequest::ListLogs {
            room_id: args.next(),
            limit: 300,
        },
        "board-create" => IpcRequest::CreateBoard {
            id: required(&mut args, "board id")?,
            room_id: required(&mut args, "room id")?,
            title: required(&mut args, "board title")?,
        },
        "board-column-create" => IpcRequest::CreateBoardColumn {
            id: required(&mut args, "column id")?,
            board_id: required(&mut args, "board id")?,
            title: required(&mut args, "column title")?,
            position: parse_i32(&required(&mut args, "position")?, "position")?,
            is_done: parse_bool(&required(&mut args, "is done")?, "is done")?,
        },
        "board-list" => IpcRequest::ListBoards {
            room_id: required(&mut args, "room id")?,
        },
        "card-create" => IpcRequest::CreateCard {
            card: CardInput {
                board_id: required(&mut args, "board id")?,
                column_id: required(&mut args, "column id")?,
                title: required(&mut args, "card title")?,
                description: None,
                position: 0,
                assignee_id: args.next(),
            },
        },
        "card-move" => IpcRequest::MoveCard {
            card_id: required(&mut args, "card id")?,
            column_id: required(&mut args, "column id")?,
            position: parse_i32(&required(&mut args, "position")?, "position")?,
        },
        "card-release" => IpcRequest::ReleaseCardClaim {
            card_id: required(&mut args, "card id")?,
            claimed_by: required(&mut args, "claimant id")?,
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
        scanner_enabled: parse_bool(&required(args, "scanner enabled")?, "scanner enabled")?,
    })
}

fn permission_reply(value: &str) -> CliResult<PermissionReply> {
    match value {
        "once" => Ok(PermissionReply::Once),
        "always" => Ok(PermissionReply::Always),
        "reject" => Ok(PermissionReply::Reject),
        _ => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "permission reply must be once, always, or reject",
        )
        .into()),
    }
}

fn required(args: &mut impl Iterator<Item = String>, name: &'static str) -> CliResult<String> {
    args.next().ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidInput, format!("missing {name}")).into()
    })
}

fn parse_i32(value: &str, name: &str) -> CliResult<i32> {
    value.parse().map_err(|error| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("invalid {name} {value:?}: {error}"),
        )
        .into()
    })
}

fn parse_bool(value: &str, name: &str) -> CliResult<bool> {
    value.parse().map_err(|error| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("invalid {name} {value:?}: {error}"),
        )
        .into()
    })
}

fn print_usage() {
    println!(
        "OpenWork collaboration\n\n\
         Usage:\n\
           openwork-collab daemon\n\
           openwork-collab status | shutdown | agent-list | permissions\n\
           openwork-collab permission-reply <permission-id> <once|always|reject> [message]\n\
           openwork-collab permission-abort <permission-id>\n\
           openwork-collab agent-create <id> <display-name> <provider-id> <model-id> <system-prompt> <scanner-enabled>\n\
           openwork-collab agent-update <id> <display-name> <provider-id> <model-id> <system-prompt> <scanner-enabled>\n\
           openwork-collab room-create <id> <title>\n\
           openwork-collab dm-create <first-participant-id> <second-participant-id>\n\
           openwork-collab room-add <room-id> <participant-id>\n\
           openwork-collab send <room-id> <author-id> <body>\n\
           openwork-collab messages <room-id>\n\
           openwork-collab credential-check <provider-id>\n\n\
           openwork-collab triage-config <provider-id> <model-id>\n\
           openwork-collab triage-list [room-id]\n\n\
           openwork-collab logs [room-id]\n\n\
           openwork-collab board-create <id> <room-id> <title>\n\
           openwork-collab board-column-create <id> <board-id> <title> <position> <is-done>\n\
           openwork-collab board-list <room-id>\n\
           openwork-collab card-create <board-id> <column-id> <title> [assignee-id]\n\
           openwork-collab card-move <card-id> <column-id> <position>\n\
           openwork-collab card-release <card-id> <claimant-id>\n\n\
         Environment:\n\
           DATABASE_URL            PostgreSQL URL\n\
           OPENWORK_COLLAB_HOME     daemon state root (default ~/.openwork/collab)\n\
           OPENWORK_COLLAB_EVENT_RETENTION_DAYS  event retention (default 30)\n\
           OPENWORK_COLLAB_TRIAGE_RETENTION_DAYS triage retention (default 30)\n\
           OPENWORK_COLLAB_GC_BATCH_SIZE          rows per delete (default 500)\n\
           OPENWORK_COLLAB_GC_STATEMENT_TIMEOUT_MS per-delete timeout (default 2000)\n\
           OPENCODE_BIN             alternate opencode executable\n\
           OPENCODE_SERVER_PASSWORD optional OpenCode basic-auth password"
    );
}
