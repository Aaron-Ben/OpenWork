use std::{fmt::Write as _, io::Read as _};

use crate::protocol::{
    AgentCommand, AgentCommandRequest, AgentCommandResponse, AgentCommandResult,
    MESSAGE_BODY_MAX_BYTES, request_id,
};

const HELP: &str = "Usage:
  openwork inbox
  openwork glance <room-id>
  openwork reply <room-id> [--held-token <token>] (--stdin | --file <path> | -- <body>)
  openwork ack <room-id>
  openwork dm <participant-id> (--stdin | --file <path> | -- <body>)
  openwork board list
  openwork card list [--board <board-id>]
  openwork card create --board <id> --column <id> --title <text> [--description <text>] [--assignee <id>]
  openwork card claim <card-id>
  openwork card move <card-id> --column <id> [--before-card <card-id>]";

pub fn main() -> i32 {
    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(error) => {
            eprintln!("could not start openwork shim: {error}");
            return 1;
        }
    };
    match runtime.block_on(run()) {
        Ok(output) => {
            println!("{}", output.text);
            output.exit_code
        }
        Err(error) => {
            eprintln!("{error}");
            1
        }
    }
}

struct ShimOutput {
    text: String,
    exit_code: i32,
}

async fn run() -> Result<ShimOutput, ShimError> {
    let arguments = std::env::args().skip(1).collect::<Vec<_>>();
    if arguments.is_empty() || arguments == ["--help"] || arguments == ["help"] {
        return Ok(ShimOutput {
            text: HELP.to_string(),
            exit_code: 0,
        });
    }
    let command = parse_command(arguments).await?;
    let base_url = std::env::var("OPENWORK_RUNTIME_BASE_URL")
        .map_err(|_| ShimError::Environment("OPENWORK_RUNTIME_BASE_URL is not set"))?;
    let token_file = std::env::var("OPENWORK_RUNTIME_TOKEN_FILE")
        .map_err(|_| ShimError::Environment("OPENWORK_RUNTIME_TOKEN_FILE is not set"))?;
    let token = tokio::fs::read_to_string(token_file).await?;
    let request = AgentCommandRequest {
        request_id: request_id(),
        command,
    };
    let client = reqwest::Client::new();
    let mut delay = std::time::Duration::from_millis(100);
    let mut attempts = 0;
    let response = loop {
        attempts += 1;
        let response = client
            .post(format!("{}/agent/commands", base_url.trim_end_matches('/')))
            .bearer_auth(token.trim())
            .json(&request)
            .timeout(std::time::Duration::from_secs(20))
            .send()
            .await
            .and_then(reqwest::Response::error_for_status);
        let response = match response {
            Ok(response) => response.json::<AgentCommandResponse>().await,
            Err(error) => Err(error),
        };
        match response {
            Ok(response) => break response,
            Err(error) if transient_http(&error) && attempts < 3 => {
                tokio::time::sleep(delay).await;
                delay *= 2;
            }
            Err(error) => return Err(error.into()),
        }
    };
    render(response)
}

fn transient_http(error: &reqwest::Error) -> bool {
    error.status().is_none_or(|status| {
        status.is_server_error()
            || status == reqwest::StatusCode::REQUEST_TIMEOUT
            || status == reqwest::StatusCode::TOO_MANY_REQUESTS
    })
}

async fn parse_command(arguments: Vec<String>) -> Result<AgentCommand, ShimError> {
    match arguments.as_slice() {
        [command] if command == "inbox" => Ok(AgentCommand::Inbox),
        [command, room_id] if command == "glance" => Ok(AgentCommand::Glance {
            room_id: room_id.clone(),
        }),
        [command, room_id] if command == "ack" => Ok(AgentCommand::Ack {
            room_id: room_id.clone(),
        }),
        [board, action] if board == "board" && action == "list" => Ok(AgentCommand::BoardList),
        [card, action] if card == "card" && action == "list" => {
            Ok(AgentCommand::CardList { board_id: None })
        }
        [card, action, flag, board_id]
            if card == "card" && action == "list" && flag == "--board" =>
        {
            Ok(AgentCommand::CardList {
                board_id: Some(board_id.clone()),
            })
        }
        [card, action, card_id] if card == "card" && action == "claim" => {
            Ok(AgentCommand::CardClaim {
                card_id: card_id.clone(),
            })
        }
        [command, participant_id, tail @ ..] if command == "dm" => {
            Ok(AgentCommand::DirectMessage {
                participant_id: participant_id.clone(),
                body: parse_body(tail).await?,
            })
        }
        [command, room_id, tail @ ..] if command == "reply" => {
            let (held_token, body_arguments) = if let [flag, token, rest @ ..] = tail {
                if flag == "--held-token" {
                    (Some(token.clone()), rest)
                } else {
                    (None, tail)
                }
            } else {
                (None, tail)
            };
            Ok(AgentCommand::Reply {
                room_id: room_id.clone(),
                body: parse_body(body_arguments).await?,
                held_token,
            })
        }
        [card, action, tail @ ..] if card == "card" && action == "create" => {
            let flags = parse_flags(
                tail,
                &[
                    "--board",
                    "--column",
                    "--title",
                    "--description",
                    "--assignee",
                ],
            )?;
            Ok(AgentCommand::CardCreate {
                board_id: required_flag(&flags, "--board")?.to_string(),
                column_id: required_flag(&flags, "--column")?.to_string(),
                title: required_flag(&flags, "--title")?.to_string(),
                description: flags.get("--description").cloned(),
                assignee_id: flags.get("--assignee").cloned(),
            })
        }
        [card, action, card_id, tail @ ..] if card == "card" && action == "move" => {
            let flags = parse_flags(tail, &["--column", "--before-card"])?;
            Ok(AgentCommand::CardMove {
                card_id: card_id.clone(),
                column_id: required_flag(&flags, "--column")?.to_string(),
                before_card_id: flags.get("--before-card").cloned(),
            })
        }
        _ => Err(ShimError::Arguments(format!("unknown command\n\n{HELP}"))),
    }
}

fn parse_flags(
    arguments: &[String],
    allowed: &[&str],
) -> Result<std::collections::BTreeMap<String, String>, ShimError> {
    if !arguments.len().is_multiple_of(2) {
        return Err(ShimError::Arguments(
            "each option requires exactly one value".to_string(),
        ));
    }
    let mut flags = std::collections::BTreeMap::new();
    for pair in arguments.chunks_exact(2) {
        if !allowed.contains(&pair[0].as_str()) {
            return Err(ShimError::Arguments(format!("unknown option {}", pair[0])));
        }
        if pair[1].is_empty() || flags.insert(pair[0].clone(), pair[1].clone()).is_some() {
            return Err(ShimError::Arguments(format!(
                "option {} must appear once with a non-empty value",
                pair[0]
            )));
        }
    }
    Ok(flags)
}

fn required_flag<'a>(
    flags: &'a std::collections::BTreeMap<String, String>,
    name: &str,
) -> Result<&'a str, ShimError> {
    flags
        .get(name)
        .map(String::as_str)
        .ok_or_else(|| ShimError::Arguments(format!("missing {name}")))
}

async fn parse_body(arguments: &[String]) -> Result<String, ShimError> {
    let body = match arguments {
        [flag] if flag == "--stdin" => read_stdin_body()?,
        [flag, path] if flag == "--file" => read_file_body(path).await?,
        [separator, body] if separator == "--" => body.clone(),
        _ => {
            return Err(ShimError::Arguments(
                "body requires --stdin, --file <path>, or -- <body>".to_string(),
            ));
        }
    };
    if body.len() > MESSAGE_BODY_MAX_BYTES {
        return Err(ShimError::Arguments(format!(
            "body exceeds {MESSAGE_BODY_MAX_BYTES} bytes"
        )));
    }
    Ok(body)
}

fn render(response: AgentCommandResponse) -> Result<ShimOutput, ShimError> {
    let (text, exit_code) = match response.result {
        AgentCommandResult::Error { code, message } => (format!("{code}: {message}"), 2),
        AgentCommandResult::Held {
            room_id,
            retry_token,
            messages,
        } => {
            let mut text = format!(
                "HELD: room {room_id} changed; reconsider, then retry with --held-token {retry_token}"
            );
            render_messages(&mut text, &messages);
            (text, 10)
        }
        AgentCommandResult::Inbox {
            carried_over,
            messages,
        } => {
            let mut text = if carried_over {
                "Inbox (more unread messages remain)".to_string()
            } else {
                "Inbox".to_string()
            };
            render_messages(&mut text, &messages);
            (text, 0)
        }
        AgentCommandResult::Glance {
            room_id,
            compose_anchor,
            members,
            messages,
        } => {
            let names = members
                .iter()
                .map(|member| format!("{} ({})", member.display_name, member.id))
                .collect::<Vec<_>>()
                .join(", ");
            let mut text = format!("Room {room_id} at {compose_anchor}\nMembers: {names}");
            render_messages(&mut text, &messages);
            (text, 0)
        }
        AgentCommandResult::MessagePublished { message } => (
            format!(
                "Published {} in {} at sequence {}",
                message.id, message.room_id, message.sequence
            ),
            0,
        ),
        AgentCommandResult::Acknowledged { room_id, up_to_seq } => {
            (format!("Acknowledged {room_id} through {up_to_seq}"), 0)
        }
        AgentCommandResult::DirectMessageSent { room_id, message } => (
            format!(
                "Published {} in direct room {room_id} at sequence {}",
                message.id, message.sequence
            ),
            0,
        ),
        AgentCommandResult::Boards { boards } => (
            serde_json::to_string_pretty(&boards).map_err(ShimError::Json)?,
            0,
        ),
        AgentCommandResult::Cards { cards } => (
            serde_json::to_string_pretty(&cards).map_err(ShimError::Json)?,
            0,
        ),
        AgentCommandResult::Card { card } => (
            serde_json::to_string_pretty(&card).map_err(ShimError::Json)?,
            0,
        ),
    };
    Ok(ShimOutput { text, exit_code })
}

fn render_messages(output: &mut String, messages: &[crate::protocol::MessageView]) {
    if messages.is_empty() {
        output.push_str("\n(no messages)");
    }
    for message in messages {
        let _ = write!(
            output,
            "\n[{}] {} @ {}: {}",
            message.sequence, message.author_id, message.room_id, message.body
        );
    }
}

fn read_stdin_body() -> Result<String, ShimError> {
    let mut bytes = Vec::new();
    std::io::stdin()
        .take((MESSAGE_BODY_MAX_BYTES + 1) as u64)
        .read_to_end(&mut bytes)?;
    if bytes.len() > MESSAGE_BODY_MAX_BYTES {
        return Err(ShimError::Arguments(format!(
            "body exceeds {MESSAGE_BODY_MAX_BYTES} bytes"
        )));
    }
    String::from_utf8(bytes)
        .map_err(|_| ShimError::Arguments("body must be valid UTF-8".to_string()))
}

async fn read_file_body(path: &str) -> Result<String, ShimError> {
    let home = std::env::var("OPENWORK_AGENT_HOME")
        .map_err(|_| ShimError::Environment("OPENWORK_AGENT_HOME is not set"))?;
    let home = tokio::fs::canonicalize(home).await?;
    let path = tokio::fs::canonicalize(path).await?;
    if !path.starts_with(&home) {
        return Err(ShimError::Arguments(
            "--file must resolve inside OPENWORK_AGENT_HOME".to_string(),
        ));
    }
    let metadata = tokio::fs::metadata(&path).await?;
    if metadata.len() > MESSAGE_BODY_MAX_BYTES as u64 {
        return Err(ShimError::Arguments(format!(
            "body exceeds {MESSAGE_BODY_MAX_BYTES} bytes"
        )));
    }
    let bytes = tokio::fs::read(path).await?;
    String::from_utf8(bytes)
        .map_err(|_| ShimError::Arguments("body must be valid UTF-8".to_string()))
}

#[derive(Debug, thiserror::Error)]
enum ShimError {
    #[error("invalid arguments: {0}")]
    Arguments(String),
    #[error("{0}")]
    Environment(&'static str),
    #[error("shim I/O failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("Runtime request failed: {0}")]
    Http(#[from] reqwest::Error),
    #[error("Runtime result was invalid: {0}")]
    Json(serde_json::Error),
}
