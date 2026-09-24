use crate::protocol::{AgentCommandRequest, AgentCommandResponse, request_id};

mod parse;
mod render;

use parse::{output_format, parse_command};
use render::render;

const HELP: &str = "Usage:
  openwork inbox
  openwork rooms
  openwork messages <room-id> [--tail <1..200>] [--json]
  openwork members <room-id>
  openwork participants
  openwork glance <room-id>
  openwork reply <room-id> [--quote <message-id>] [--held-token <token>] [--continue] (<body> | --stdin | --file <path>)
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
  openwork card update <card-id> [--title <text>] [--description <text> | --stdin | --file <path>]
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
    let (arguments, format) = output_format(arguments);
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
    render(response, format)
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
    use super::help_request;

    /// 子命令后面的 `--help` 只显示这个子命令的用法；`--` 之后的 `--help` 是正文。
    #[test]
    fn help_after_a_subcommand_shows_only_that_usage() {
        let help = |values: &[&str]| help_request(&arguments(values));
        assert_eq!(
            help(&["reply", "--help"]).as_deref(),
            Some(
                "Usage:\n  openwork reply <room-id> [--quote <message-id>] [--held-token <token>] [--continue] (<body> | --stdin | --file <path>)"
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

    fn arguments(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| value.to_string()).collect()
    }
}
