use std::io::Read as _;

use crate::protocol::{CLI_MESSAGE_BODY_MAX_BYTES, CliRequest, CliResult};

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
        Ok(result) => {
            println!("{}", result.text);
            result.exit_code.clamp(0, 255)
        }
        Err(error) => {
            eprintln!("{error}");
            1
        }
    }
}

async fn run() -> Result<CliResult, ShimError> {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    if arguments.is_empty() || arguments == ["--help"] || arguments == ["help"] {
        return Ok(CliResult {
            text:
                "Usage:\n  openwork inbox\n  openwork glance <room-id>\n  openwork reply <room-id> [--held-token <token>] (--stdin | --file <path> | -- <body>)\n  openwork react <message-id> <emoji>\n  openwork ack <room-id>\n  openwork dm <participant-id> (--stdin | --file <path> | -- <body>)\n  openwork group create --member <participant-id>... (--stdin | --file <path> | -- <opening>)\n  openwork group invite <room-id> <participant-id>\n  openwork group leave <room-id>\n  openwork group kick <room-id> <participant-id>\n  openwork card list --room <room-id>\n  openwork card create --board <id> --column <id> --title <text> [--assignee <id>]\n  openwork card claim <card-id>\n  openwork card move <card-id> --column <id> --position <n>"
                    .to_string(),
            exit_code: 0,
            side_effects: Vec::new(),
        });
    }
    let argv = normalize_arguments(arguments).await?;
    let base_url = std::env::var("OPENWORK_RUNTIME_BASE_URL")
        .map_err(|_| ShimError::Environment("OPENWORK_RUNTIME_BASE_URL is not set"))?;
    let token_file = std::env::var("OPENWORK_RUNTIME_TOKEN_FILE")
        .map_err(|_| ShimError::Environment("OPENWORK_RUNTIME_TOKEN_FILE is not set"))?;
    let token = tokio::fs::read_to_string(token_file).await?;
    let response = reqwest::Client::new()
        .post(format!("{}/runtime/cli", base_url.trim_end_matches('/')))
        .bearer_auth(token.trim())
        .json(&CliRequest {
            request_id: format!("cli_{}", uuid::Uuid::new_v4().simple()),
            argv,
        })
        .send()
        .await?
        .error_for_status()?;
    Ok(response.json().await?)
}

async fn normalize_arguments(arguments: Vec<String>) -> Result<Vec<String>, ShimError> {
    if valid_bodyless_command(&arguments) {
        return Ok(arguments);
    }
    let (mut prefix, body) = if arguments
        .last()
        .is_some_and(|argument| argument == "--stdin")
    {
        let mut prefix = arguments;
        prefix.pop();
        (prefix, read_stdin_body()?)
    } else if arguments.len() >= 2 && arguments[arguments.len() - 2] == "--file" {
        let mut prefix = arguments;
        let path = prefix.pop().expect("checked length");
        prefix.pop();
        (prefix, read_file_body(&path).await?)
    } else if arguments.len() >= 2 && arguments[arguments.len() - 2] == "--" {
        let mut prefix = arguments;
        let body = prefix.pop().expect("checked length");
        prefix.pop();
        (prefix, body)
    } else {
        return Err(ShimError::Arguments(
            "a body command requires --stdin, --file <path>, or -- <body>".to_string(),
        ));
    };
    if !valid_body_command_prefix(&prefix) {
        return Err(ShimError::Arguments(
            "invalid reply, dm, or group create arguments".to_string(),
        ));
    }
    if body.len() > CLI_MESSAGE_BODY_MAX_BYTES {
        return Err(ShimError::Arguments(format!(
            "body exceeds {CLI_MESSAGE_BODY_MAX_BYTES} bytes"
        )));
    }
    prefix.push("--".to_string());
    prefix.push(body);
    Ok(prefix)
}

fn valid_bodyless_command(arguments: &[String]) -> bool {
    matches!(arguments, [command] if command == "inbox")
        || matches!(arguments, [command, _] if matches!(command.as_str(), "glance" | "ack"))
        || matches!(arguments, [command, _, _] if command == "react")
        || matches!(arguments, [group, action, _] if group == "group" && action == "leave")
        || matches!(arguments, [group, action, _, _]
            if group == "group" && matches!(action.as_str(), "invite" | "kick"))
        || matches!(arguments, [card, action, _]
            if card == "card" && action == "claim")
        || matches!(arguments, [card, action, room_flag, _]
            if card == "card" && action == "list" && room_flag == "--room")
        || (arguments.starts_with(&["card".to_string(), "create".to_string()])
            && arguments.len() >= 8
            && arguments.len().is_multiple_of(2))
        || matches!(arguments, [card, action, _, column_flag, _, position_flag, _]
            if card == "card" && action == "move"
                && column_flag == "--column" && position_flag == "--position")
}

fn valid_body_command_prefix(arguments: &[String]) -> bool {
    matches!(arguments, [command, _] if matches!(command.as_str(), "reply" | "dm"))
        || matches!(arguments, [command, _, held, token]
            if command == "reply" && held == "--held-token" && !token.is_empty())
        || (arguments.starts_with(&["group".to_string(), "create".to_string()])
            && arguments.len() >= 4
            && (arguments.len() - 2) % 2 == 0
            && arguments[2..]
                .chunks_exact(2)
                .all(|pair| pair[0] == "--member" && !pair[1].is_empty()))
}

fn read_stdin_body() -> Result<String, ShimError> {
    let mut bytes = Vec::new();
    std::io::stdin()
        .take((CLI_MESSAGE_BODY_MAX_BYTES + 1) as u64)
        .read_to_end(&mut bytes)?;
    if bytes.len() > CLI_MESSAGE_BODY_MAX_BYTES {
        return Err(ShimError::Arguments(format!(
            "body exceeds {CLI_MESSAGE_BODY_MAX_BYTES} bytes"
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
    if metadata.len() > CLI_MESSAGE_BODY_MAX_BYTES as u64 {
        return Err(ShimError::Arguments(format!(
            "body exceeds {CLI_MESSAGE_BODY_MAX_BYTES} bytes"
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
}
