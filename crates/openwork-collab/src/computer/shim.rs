use std::io::Read as _;

use crate::protocol::{CliRequest, CliResult};

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
                "Usage:\n  openwork reply <room-id> (--stdin | -- <body>)\n  openwork ack <room-id>"
                    .to_string(),
            exit_code: 0,
            side_effects: Vec::new(),
        });
    }
    let argv = normalize_arguments(arguments)?;
    let base_url = std::env::var("OPENWORK_RUNTIME_BASE_URL")
        .map_err(|_| ShimError::Environment("OPENWORK_RUNTIME_BASE_URL is not set"))?;
    let token_file = std::env::var("OPENWORK_RUNTIME_TOKEN_FILE")
        .map_err(|_| ShimError::Environment("OPENWORK_RUNTIME_TOKEN_FILE is not set"))?;
    let token = tokio::fs::read_to_string(token_file).await?;
    let response = reqwest::Client::new()
        .post(format!("{}/runtime/cli", base_url.trim_end_matches('/')))
        .bearer_auth(token.trim())
        .json(&CliRequest { argv })
        .send()
        .await?
        .error_for_status()?;
    Ok(response.json().await?)
}

fn normalize_arguments(arguments: Vec<String>) -> Result<Vec<String>, ShimError> {
    match arguments.as_slice() {
        [command, room_id, transport] if command == "reply" && transport == "--stdin" => {
            let mut body = String::new();
            std::io::stdin()
                .take(1024 * 1024)
                .read_to_string(&mut body)?;
            Ok(vec![
                "reply".to_string(),
                room_id.clone(),
                "--".to_string(),
                body,
            ])
        }
        [command, room_id, separator, body] if command == "reply" && separator == "--" => Ok(vec![
            "reply".to_string(),
            room_id.clone(),
            "--".to_string(),
            body.clone(),
        ]),
        [command, room_id] if command == "ack" => Ok(vec!["ack".to_string(), room_id.clone()]),
        _ => Err(ShimError::Arguments(
            "expected reply <room-id> (--stdin | -- <body>) or ack <room-id>".to_string(),
        )),
    }
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
