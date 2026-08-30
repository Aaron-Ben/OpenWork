#![cfg(unix)]

use std::sync::{Arc, Mutex};

use axum::{Json, Router, extract::State, routing::post};
use openwork_collab::protocol::{CliRequest, CliResult};
use tokio::{io::AsyncWriteExt, process::Command};
use tokio_util::sync::CancellationToken;

#[tokio::test]
async fn shim_transports_p2_bodies_and_held_tokens_as_literal_arguments() {
    let requests = Arc::new(Mutex::new(Vec::<CliRequest>::new()));
    let app = Router::new()
        .route("/runtime/cli", post(capture_cli))
        .with_state(requests.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let shutdown = CancellationToken::new();
    let server_shutdown = shutdown.clone();
    let server = tokio::spawn(async move {
        axum::serve(listener, app)
            .with_graceful_shutdown(server_shutdown.cancelled_owned())
            .await
            .unwrap();
    });
    let home = tempfile::tempdir().unwrap();
    let token_file = home.path().join("runtime-token");
    tokio::fs::write(&token_file, "test-token").await.unwrap();
    let body_file = home.path().join("dm-body.txt");
    tokio::fs::write(&body_file, "file `$()` body\n")
        .await
        .unwrap();

    run_shim(
        address,
        home.path(),
        &token_file,
        &[
            "reply",
            "room_group",
            "--held-token",
            "hold_once",
            "--stdin",
        ],
        Some("stdin `$(touch nope)` body\n"),
    )
    .await;
    run_shim(
        address,
        home.path(),
        &token_file,
        &[
            "card",
            "create",
            "--board",
            "board_1",
            "--column",
            "column_1",
            "--title",
            "Ship P3",
            "--assignee",
            "beta",
        ],
        None,
    )
    .await;
    run_shim(
        address,
        home.path(),
        &token_file,
        &[
            "card",
            "move",
            "card_1",
            "--column",
            "column_2",
            "--position",
            "0",
        ],
        None,
    )
    .await;
    run_shim(
        address,
        home.path(),
        &token_file,
        &["dm", "beta", "--file", body_file.to_str().unwrap()],
        None,
    )
    .await;
    run_shim(
        address,
        home.path(),
        &token_file,
        &[
            "group", "create", "--member", "user", "--member", "beta", "--stdin",
        ],
        Some("group opening"),
    )
    .await;

    {
        let requests = requests.lock().unwrap();
        assert_eq!(requests.len(), 5);
        assert!(
            requests
                .iter()
                .all(|request| request.request_id.starts_with("cli_"))
        );
        assert_eq!(
            requests[0].argv,
            [
                "reply",
                "room_group",
                "--held-token",
                "hold_once",
                "--",
                "stdin `$(touch nope)` body\n",
            ]
        );
        assert_eq!(
            requests[1].argv,
            [
                "card",
                "create",
                "--board",
                "board_1",
                "--column",
                "column_1",
                "--title",
                "Ship P3",
                "--assignee",
                "beta",
            ]
        );
        assert_eq!(
            requests[2].argv,
            [
                "card",
                "move",
                "card_1",
                "--column",
                "column_2",
                "--position",
                "0"
            ]
        );
        assert_eq!(requests[3].argv, ["dm", "beta", "--", "file `$()` body\n"]);
        assert_eq!(
            requests[4].argv,
            [
                "group",
                "create",
                "--member",
                "user",
                "--member",
                "beta",
                "--",
                "group opening",
            ]
        );
    }
    shutdown.cancel();
    server.await.unwrap();
}

async fn capture_cli(
    State(requests): State<Arc<Mutex<Vec<CliRequest>>>>,
    Json(request): Json<CliRequest>,
) -> Json<CliResult> {
    requests.lock().unwrap().push(request);
    Json(CliResult {
        text: "ok".to_string(),
        exit_code: 0,
        side_effects: Vec::new(),
    })
}

async fn run_shim(
    address: std::net::SocketAddr,
    home: &std::path::Path,
    token_file: &std::path::Path,
    arguments: &[&str],
    stdin: Option<&str>,
) {
    let mut command = Command::new(env!("CARGO_BIN_EXE_openwork"));
    command
        .args(arguments)
        .env("OPENWORK_RUNTIME_BASE_URL", format!("http://{address}"))
        .env("OPENWORK_RUNTIME_TOKEN_FILE", token_file)
        .env("OPENWORK_AGENT_HOME", home)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    if stdin.is_some() {
        command.stdin(std::process::Stdio::piped());
    }
    let mut child = command.spawn().unwrap();
    if let Some(stdin) = stdin {
        child
            .stdin
            .take()
            .unwrap()
            .write_all(stdin.as_bytes())
            .await
            .unwrap();
    }
    let output = child.wait_with_output().await.unwrap();
    assert!(
        output.status.success(),
        "shim failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}
