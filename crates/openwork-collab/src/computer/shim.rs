use std::{fmt::Write as _, io::Read as _};

use crate::protocol::{
    AgentCommand, AgentCommandRequest, AgentCommandResponse, AgentCommandResult,
    MESSAGE_BODY_MAX_BYTES, request_id,
};

const HELP: &str = "Usage:
  openwork inbox
  openwork rooms
  openwork messages <room-id> [--tail <1..200>]
  openwork members <room-id>
  openwork participants
  openwork glance <room-id>
  openwork reply <room-id> [--held-token <token>] (<body> | --stdin | --file <path>)
  openwork ack <room-id>
  openwork dm <participant-id> (<body> | --stdin | --file <path>)
  openwork climate show [participant-id]
  openwork climate note <participant-id> --affinity <-1..1> --trust <-1..1> (--stdin | --file <path> | -- <note>)
  openwork board list
  openwork board show <board-id>
  openwork card list [--board <board-id>]
  openwork card show <card-id>
  openwork card create --board <id> --column <id> --title <text> [--description <text>] [--assignee <id>]
  openwork card claim <card-id>
  openwork card assign <card-id> <participant-id>
  openwork card update <card-id> --title <text> [--description <text>]
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
    if let Some(text) = help_request(&arguments) {
        return Ok(ShimOutput { text, exit_code: 0 });
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

/// 没有参数、`help`，或 `--` 之前出现 `--help` / `-h` 时返回帮助文本。子命令后面的
/// `--help` 只返回匹配最长前缀的那几行用法，找不到时返回全部用法。
fn help_request(arguments: &[String]) -> Option<String> {
    let options = arguments
        .iter()
        .take_while(|argument| argument.as_str() != "--")
        .collect::<Vec<_>>();
    let asked = arguments.is_empty()
        || arguments == ["help"]
        || options
            .iter()
            .any(|argument| matches!(argument.as_str(), "--help" | "-h"));
    if !asked {
        return None;
    }
    let words = options
        .iter()
        .take_while(|argument| !argument.starts_with('-'))
        .map(|argument| argument.as_str())
        .collect::<Vec<_>>();
    for length in (1..=words.len()).rev() {
        let prefix = format!("  openwork {}", words[..length].join(" "));
        let lines = HELP
            .lines()
            .filter(|line| {
                line.strip_prefix(&prefix)
                    .is_some_and(|rest| rest.is_empty() || rest.starts_with(' '))
            })
            .collect::<Vec<_>>();
        if !lines.is_empty() {
            return Some(format!("Usage:\n{}", lines.join("\n")));
        }
    }
    Some(HELP.to_string())
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
        [command] if command == "rooms" => Ok(AgentCommand::Rooms),
        [command, room_id] if command == "messages" => Ok(AgentCommand::Messages {
            room_id: room_id.clone(),
            tail: 50,
        }),
        [command, room_id, flag, tail] if command == "messages" && flag == "--tail" => {
            Ok(AgentCommand::Messages {
                room_id: room_id.clone(),
                tail: parse_tail(tail)?,
            })
        }
        [command, room_id] if command == "members" => Ok(AgentCommand::Members {
            room_id: room_id.clone(),
        }),
        [command] if command == "participants" => Ok(AgentCommand::Participants),
        [command, room_id] if command == "glance" => Ok(AgentCommand::Glance {
            room_id: room_id.clone(),
        }),
        [command, room_id] if command == "ack" => Ok(AgentCommand::Ack {
            room_id: room_id.clone(),
        }),
        [board, action] if board == "board" && action == "list" => Ok(AgentCommand::BoardList),
        [board, action, board_id] if board == "board" && action == "show" => {
            Ok(AgentCommand::BoardShow {
                board_id: board_id.clone(),
            })
        }
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
        [card, action, card_id] if card == "card" && action == "show" => {
            Ok(AgentCommand::CardShow {
                card_id: card_id.clone(),
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
        [climate, action] if climate == "climate" && action == "show" => {
            Ok(AgentCommand::ClimateShow {
                participant_id: None,
            })
        }
        [climate, action, participant_id] if climate == "climate" && action == "show" => {
            Ok(AgentCommand::ClimateShow {
                participant_id: Some(participant_id.clone()),
            })
        }
        [climate, action, participant_id, tail @ ..]
            if climate == "climate" && action == "note" =>
        {
            let body_start = tail
                .iter()
                .position(|argument| matches!(argument.as_str(), "--stdin" | "--file" | "--"))
                .ok_or_else(|| {
                    ShimError::Arguments(
                        "Climate note requires --stdin, --file <path>, or -- <note>".to_string(),
                    )
                })?;
            let flags = parse_flags(&tail[..body_start], &["--affinity", "--trust"])?;
            Ok(AgentCommand::ClimateNote {
                participant_id: participant_id.clone(),
                affinity: parse_score(required_flag(&flags, "--affinity")?, "affinity")?,
                trust: parse_score(required_flag(&flags, "--trust")?, "trust")?,
                note: parse_body(&tail[body_start..]).await?,
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
            let positional = body_arguments.first().is_none_or(|first| first != "--");
            if positional
                && body_arguments
                    .iter()
                    .any(|argument| argument == "--held-token")
            {
                return Err(ShimError::Arguments(
                    "put --held-token before the message body".to_string(),
                ));
            }
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
        [card, action, card_id, assignee_id] if card == "card" && action == "assign" => {
            Ok(AgentCommand::CardAssign {
                card_id: card_id.clone(),
                assignee_id: assignee_id.clone(),
            })
        }
        [card, action, card_id, tail @ ..] if card == "card" && action == "update" => {
            let flags = parse_flags(tail, &["--title", "--description"])?;
            Ok(AgentCommand::CardUpdate {
                card_id: card_id.clone(),
                title: required_flag(&flags, "--title")?.to_string(),
                description: flags.get("--description").cloned(),
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

fn parse_tail(value: &str) -> Result<u32, ShimError> {
    value
        .parse::<u32>()
        .ok()
        .filter(|value| (1..=200).contains(value))
        .ok_or_else(|| ShimError::Arguments("--tail must be an integer from 1 to 200".to_string()))
}

fn parse_score(value: &str, name: &str) -> Result<f64, ShimError> {
    value
        .parse::<f64>()
        .ok()
        .filter(|value| value.is_finite() && (-1.0..=1.0).contains(value))
        .ok_or_else(|| ShimError::Arguments(format!("--{name} must be a number from -1 to 1")))
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

/// 正文可以直接写在 id 之后（多个参数按空格拼接，与 Cumora `reply <convo_id> "<body>"` 相同），
/// 也可以用 `--stdin` / `--file` 避开 shell 引号，或用 `--` 发送以 `--` 开头的文本。
async fn parse_body(arguments: &[String]) -> Result<String, ShimError> {
    let body = match arguments {
        [] => return Err(missing_body()),
        [flag] if flag == "--stdin" => read_stdin_body()?,
        [flag, path] if flag == "--file" => read_file_body(path).await?,
        [separator] if separator == "--" => return Err(missing_body()),
        [separator, words @ ..] if separator == "--" => words.join(" "),
        [first, ..] if first.starts_with("--") => {
            return Err(ShimError::Arguments(format!(
                "unknown option {first}; to send text that starts with --, put -- before it"
            )));
        }
        words => words.join(" "),
    };
    if body.len() > MESSAGE_BODY_MAX_BYTES {
        return Err(ShimError::Arguments(format!(
            "body exceeds {MESSAGE_BODY_MAX_BYTES} bytes"
        )));
    }
    Ok(body)
}

fn missing_body() -> ShimError {
    ShimError::Arguments(
        "missing message body; write it after the id, or use --stdin or --file <path> for text with quotes or $"
            .to_string(),
    )
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
            if messages.is_empty() {
                text.push_str(&format!(
                    "\nNo new messages since you last read this room (latest sequence {compose_anchor})."
                ));
            } else {
                render_messages(&mut text, &messages);
            }
            (text, 0)
        }
        AgentCommandResult::Rooms { rooms } => (
            serde_json::to_string_pretty(&rooms).map_err(ShimError::Json)?,
            0,
        ),
        AgentCommandResult::Messages { room_id, messages } => {
            let mut text = format!("Messages in {room_id}");
            render_messages(&mut text, &messages);
            (text, 0)
        }
        AgentCommandResult::Members { room_id, members } => (
            format!(
                "Members in {room_id}\n{}",
                serde_json::to_string_pretty(&members).map_err(ShimError::Json)?
            ),
            0,
        ),
        AgentCommandResult::Participants { participants } => (
            serde_json::to_string_pretty(&participants).map_err(ShimError::Json)?,
            0,
        ),
        AgentCommandResult::Climates { climates } => (
            serde_json::to_string_pretty(&climates).map_err(ShimError::Json)?,
            0,
        ),
        AgentCommandResult::Climate { climate } => (
            serde_json::to_string_pretty(&climate).map_err(ShimError::Json)?,
            0,
        ),
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
        AgentCommandResult::Board { board } => (
            serde_json::to_string_pretty(&board).map_err(ShimError::Json)?,
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

#[cfg(test)]
mod tests {
    use super::{help_request, parse_command, parse_score, parse_tail, render};
    use crate::protocol::{
        AgentCommand, AgentCommandResponse, AgentCommandResult, ParticipantView,
    };

    /// 子命令后面的 `--help` 只显示这个子命令的用法；`--` 之后的 `--help` 是正文。
    #[test]
    fn help_after_a_subcommand_shows_only_that_usage() {
        let help = |values: &[&str]| help_request(&arguments(values));
        assert_eq!(
            help(&["reply", "--help"]).as_deref(),
            Some(
                "Usage:\n  openwork reply <room-id> [--held-token <token>] (<body> | --stdin | --file <path>)"
            )
        );
        assert_eq!(
            help(&["card", "move", "-h"]).as_deref(),
            Some("Usage:\n  openwork card move <card-id> --column <id> [--before-card <card-id>]")
        );
        assert_eq!(
            help(&["glance", "room-1", "--help"]).as_deref(),
            Some("Usage:\n  openwork glance <room-id>")
        );
        assert_eq!(help(&["reply", "room-1", "--", "--help"]), None);
        assert_eq!(help(&["reply", "room-1", "Ship it."]), None);
        assert!(help(&["--help"]).unwrap().contains("  openwork card move"));
        assert!(
            help(&["frobnicate", "--help"])
                .unwrap()
                .contains("  openwork inbox")
        );
    }

    /// glance 没有新消息时说明原因，而不是只写 `(no messages)`。
    #[test]
    fn empty_glance_says_nothing_new_since_the_last_read() {
        let output = render(AgentCommandResponse {
            result: AgentCommandResult::Glance {
                room_id: "room-1".to_string(),
                compose_anchor: 3,
                members: vec![ParticipantView {
                    id: "bo".to_string(),
                    kind: "agent".to_string(),
                    display_name: "Bo".to_string(),
                }],
                messages: Vec::new(),
            },
            effects: Vec::new(),
        })
        .unwrap();

        assert_eq!(
            output.text,
            "Room room-1 at 3\nMembers: Bo (bo)\nNo new messages since you last read this room (latest sequence 3)."
        );
    }

    #[tokio::test]
    async fn parses_read_and_climate_commands() {
        assert_eq!(
            parse_command(vec!["rooms".to_string()]).await.unwrap(),
            AgentCommand::Rooms
        );
        assert_eq!(
            parse_command(vec![
                "messages".to_string(),
                "room-1".to_string(),
                "--tail".to_string(),
                "25".to_string(),
            ])
            .await
            .unwrap(),
            AgentCommand::Messages {
                room_id: "room-1".to_string(),
                tail: 25,
            }
        );
        assert_eq!(
            parse_command(vec![
                "climate".to_string(),
                "note".to_string(),
                "beta".to_string(),
                "--affinity".to_string(),
                "0.75".to_string(),
                "--trust".to_string(),
                "-0.25".to_string(),
                "--".to_string(),
                "Strong technically; verify estimates.".to_string(),
            ])
            .await
            .unwrap(),
            AgentCommand::ClimateNote {
                participant_id: "beta".to_string(),
                affinity: 0.75,
                trust: -0.25,
                note: "Strong technically; verify estimates.".to_string(),
            }
        );
    }

    fn arguments(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| value.to_string()).collect()
    }

    fn reply(body: &str, held_token: Option<&str>) -> AgentCommand {
        AgentCommand::Reply {
            room_id: "room-1".to_string(),
            body: body.to_string(),
            held_token: held_token.map(str::to_string),
        }
    }

    /// 正文可以直接跟在房间 id 后面（Cumora `reply <convo_id> "<body>"`）；多个参数按空格拼接，
    /// `--` 仍然可用。
    #[tokio::test]
    async fn reply_and_dm_accept_the_body_as_plain_arguments() {
        assert_eq!(
            parse_command(arguments(&["reply", "room-1", "Ship it today."]))
                .await
                .unwrap(),
            reply("Ship it today.", None)
        );
        assert_eq!(
            parse_command(arguments(&["reply", "room-1", "Ship", "it", "today."]))
                .await
                .unwrap(),
            reply("Ship it today.", None)
        );
        assert_eq!(
            parse_command(arguments(&["reply", "room-1", "--", "--not-a-flag"]))
                .await
                .unwrap(),
            reply("--not-a-flag", None)
        );
        assert_eq!(
            parse_command(arguments(&[
                "reply",
                "room-1",
                "--held-token",
                "hold-1",
                "Still needed."
            ]))
            .await
            .unwrap(),
            reply("Still needed.", Some("hold-1"))
        );
        assert_eq!(
            parse_command(arguments(&["dm", "bo", "Can you review card-7?"]))
                .await
                .unwrap(),
            AgentCommand::DirectMessage {
                participant_id: "bo".to_string(),
                body: "Can you review card-7?".to_string(),
            }
        );
    }

    /// 缺正文或把选项写在正文之后时，拒绝并告诉模型正确写法（模型可见文本逐字断言）。
    #[tokio::test]
    async fn misplaced_options_and_missing_bodies_are_rejected_with_the_usage() {
        assert_eq!(
            parse_command(arguments(&["reply", "room-1"]))
                .await
                .unwrap_err()
                .to_string(),
            "invalid arguments: missing message body; write it after the id, or use --stdin or --file <path> for text with quotes or $"
        );
        assert_eq!(
            parse_command(arguments(&[
                "reply",
                "room-1",
                "Still needed.",
                "--held-token",
                "hold-1"
            ]))
            .await
            .unwrap_err()
            .to_string(),
            "invalid arguments: put --held-token before the message body"
        );
        assert_eq!(
            parse_command(arguments(&["reply", "room-1", "--tail", "5"]))
                .await
                .unwrap_err()
                .to_string(),
            "invalid arguments: unknown option --tail; to send text that starts with --, put -- before it"
        );
    }

    #[tokio::test]
    async fn parses_the_complete_card_surface_without_structure_or_delete_commands() {
        assert_eq!(
            parse_command(vec![
                "board".to_string(),
                "show".to_string(),
                "board-1".to_string(),
            ])
            .await
            .unwrap(),
            AgentCommand::BoardShow {
                board_id: "board-1".to_string(),
            }
        );
        assert_eq!(
            parse_command(vec![
                "card".to_string(),
                "assign".to_string(),
                "card-1".to_string(),
                "alpha".to_string(),
            ])
            .await
            .unwrap(),
            AgentCommand::CardAssign {
                card_id: "card-1".to_string(),
                assignee_id: "alpha".to_string(),
            }
        );
        assert_eq!(
            parse_command(vec![
                "card".to_string(),
                "update".to_string(),
                "card-1".to_string(),
                "--title".to_string(),
                "Revised".to_string(),
            ])
            .await
            .unwrap(),
            AgentCommand::CardUpdate {
                card_id: "card-1".to_string(),
                title: "Revised".to_string(),
                description: None,
            }
        );
        for forbidden in [
            vec!["board", "delete", "board-1"],
            vec!["column", "create", "Review"],
            vec!["card", "delete", "card-1"],
        ] {
            assert!(
                parse_command(forbidden.into_iter().map(str::to_string).collect())
                    .await
                    .is_err()
            );
        }
    }

    #[test]
    fn bounds_numeric_arguments() {
        assert_eq!(parse_tail("1").unwrap(), 1);
        assert_eq!(parse_tail("200").unwrap(), 200);
        assert!(parse_tail("0").is_err());
        assert!(parse_tail("201").is_err());
        assert_eq!(parse_score("-1", "trust").unwrap(), -1.0);
        assert_eq!(parse_score("1", "trust").unwrap(), 1.0);
        assert!(parse_score("NaN", "trust").is_err());
        assert!(parse_score("1.1", "trust").is_err());
    }
}
