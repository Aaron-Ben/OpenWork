#![cfg(unix)]

use std::{os::unix::fs::PermissionsExt, path::PathBuf};

use openwork_collab::computer::engine::{
    ClassifyRequest, EngineAdapter, EngineAvailability, EngineError, TurnRequest,
};
use openwork_collab::computer::opencode::OpenCodeAdapter;
use tempfile::TempDir;
use tokio_util::sync::CancellationToken;

async fn fake_opencode(directory: &TempDir) -> PathBuf {
    let executable = directory.path().join("opencode");
    tokio::fs::write(
        &executable,
        r#"#!/bin/sh
prompt=$(cat)
if [ "$prompt" != "continue the work" ]; then
  echo '{"type":"error","error":{"message":"prompt was not delivered through stdin"}}'
  exit 0
fi
printf '%s\n' \
  '{"type":"step_start","sessionID":"ses_local"}' \
  '{"type":"text","sessionID":"ses_local","part":{"text":"done"}}' \
  '{"type":"step_finish","sessionID":"ses_local","part":{"tokens":{"input":11,"output":3,"reasoning":2,"cache":{"read":7,"write":5}}}}'
"#,
    )
    .await
    .unwrap();
    let mut permissions = tokio::fs::metadata(&executable)
        .await
        .unwrap()
        .permissions();
    permissions.set_mode(0o700);
    tokio::fs::set_permissions(&executable, permissions)
        .await
        .unwrap();
    executable
}

#[tokio::test]
async fn opencode_run_turn_uses_stdin_and_returns_resumable_structured_result() {
    let directory = tempfile::tempdir().unwrap();
    let executable = fake_opencode(&directory).await;
    let adapter = OpenCodeAdapter::with_executable(executable);

    let result = adapter
        .run_turn(TurnRequest {
            home: directory.path().to_path_buf(),
            prompt: "continue the work".to_string(),
            model: Some("openai/gpt-5".to_string()),
            resume_session_id: Some("ses_previous".to_string()),
            environment: Default::default(),
            cancellation: CancellationToken::new(),
        })
        .await
        .unwrap();

    assert_eq!(result.text, "done");
    assert_eq!(result.session_id.as_deref(), Some("ses_local"));
    assert_eq!(result.usage.input_tokens, 11);
    assert_eq!(result.usage.output_tokens, 5);
    assert_eq!(result.usage.cached_input_tokens, 7);
    assert_eq!(result.usage.cache_creation_input_tokens, 5);
}

#[tokio::test]
async fn opencode_nonzero_exit_preserves_a_reported_json_error() {
    let directory = tempfile::tempdir().unwrap();
    let executable = directory.path().join("opencode-provider-error");
    tokio::fs::write(
        &executable,
        r#"#!/bin/sh
cat >/dev/null
echo '{"type":"error","error":{"data":{"message":"Insufficient balance"}}}'
exit 1
"#,
    )
    .await
    .unwrap();
    let mut permissions = tokio::fs::metadata(&executable)
        .await
        .unwrap()
        .permissions();
    permissions.set_mode(0o700);
    tokio::fs::set_permissions(&executable, permissions)
        .await
        .unwrap();

    let result = OpenCodeAdapter::with_executable(executable)
        .run_turn(TurnRequest {
            home: directory.path().to_path_buf(),
            prompt: "reply".to_string(),
            model: None,
            resume_session_id: None,
            environment: Default::default(),
            cancellation: CancellationToken::new(),
        })
        .await;

    assert!(matches!(
        result,
        Err(EngineError::Reported(message)) if message == "Insufficient balance"
    ));
}

#[tokio::test]
async fn reported_rate_limit_terminates_a_still_running_opencode_process() {
    let directory = tempfile::tempdir().unwrap();
    let executable = directory.path().join("opencode-rate-limited");
    tokio::fs::write(
        &executable,
        r#"#!/bin/sh
cat >/dev/null
echo '{"type":"error","error":{"data":{"message":"Rate limit exceeded. Please try again later."}}}'
sleep 60
"#,
    )
    .await
    .unwrap();
    let mut permissions = tokio::fs::metadata(&executable)
        .await
        .unwrap()
        .permissions();
    permissions.set_mode(0o700);
    tokio::fs::set_permissions(&executable, permissions)
        .await
        .unwrap();

    let result = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        OpenCodeAdapter::with_executable(executable).run_turn(TurnRequest {
            home: directory.path().to_path_buf(),
            prompt: "reply".to_string(),
            model: None,
            resume_session_id: None,
            environment: Default::default(),
            cancellation: CancellationToken::new(),
        }),
    )
    .await
    .expect("reported errors must not wait for the no-output timeout");

    assert!(matches!(
        result,
        Err(EngineError::Reported(message)) if message.contains("Rate limit exceeded")
    ));
}

#[tokio::test]
async fn opencode_classify_runs_the_reserved_agent_with_every_tool_denied() {
    let directory = tempfile::tempdir().unwrap();
    let executable = directory.path().join("opencode-classify");
    tokio::fs::write(
        &executable,
        r#"#!/bin/sh
case " $* " in
  *" --agent openwork-triage "*) ;;
  *) echo '{"type":"error","error":{"message":"missing reserved triage agent"}}'; exit 0 ;;
esac
case "$OPENCODE_CONFIG_CONTENT" in
  *'"openwork-triage"'*'"*":"deny"'*) ;;
  *) echo '{"type":"error","error":{"message":"triage tools are not denied"}}'; exit 0 ;;
esac
prompt=$(cat)
[ "$prompt" = '{"actionable":false}' ] || exit 9
printf '%s\n' \
  '{"type":"step_start","sessionID":"ses_triage"}' \
  '{"type":"text","part":{"text":"{\"actionable\":false}"}}'
"#,
    )
    .await
    .unwrap();
    let mut permissions = tokio::fs::metadata(&executable)
        .await
        .unwrap()
        .permissions();
    permissions.set_mode(0o700);
    tokio::fs::set_permissions(&executable, permissions)
        .await
        .unwrap();
    let adapter = OpenCodeAdapter::with_executable(executable);

    let result = adapter
        .classify(ClassifyRequest {
            cwd: directory.path().to_path_buf(),
            prompt: r#"{"actionable":false}"#.to_string(),
            model: None,
            environment: Default::default(),
            cancellation: CancellationToken::new(),
        })
        .await
        .unwrap();

    assert_eq!(result.text, r#"{"actionable":false}"#);
}

#[tokio::test]
async fn cancellation_terminates_the_entire_opencode_process_group() {
    let directory = tempfile::tempdir().unwrap();
    let executable = directory.path().join("opencode-hung");
    tokio::fs::write(
        &executable,
        r#"#!/bin/sh
pid_file=$(cat)
echo $$ > "$pid_file"
sleep 60 &
echo $! >> "$pid_file"
wait
"#,
    )
    .await
    .unwrap();
    let mut permissions = tokio::fs::metadata(&executable)
        .await
        .unwrap()
        .permissions();
    permissions.set_mode(0o700);
    tokio::fs::set_permissions(&executable, permissions)
        .await
        .unwrap();
    let adapter = OpenCodeAdapter::with_executable(executable);
    let cancellation = CancellationToken::new();
    let pid_file = directory.path().join("pids");
    let pid_file_for_task = pid_file.clone();
    let home = directory.path().to_path_buf();
    let task_cancellation = cancellation.clone();
    let task = tokio::spawn(async move {
        adapter
            .run_turn(TurnRequest {
                home,
                prompt: pid_file_for_task.to_string_lossy().into_owned(),
                model: None,
                resume_session_id: None,
                environment: Default::default(),
                cancellation: task_cancellation,
            })
            .await
    });
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        while !tokio::fs::try_exists(&pid_file).await.unwrap() {
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    let pids = tokio::fs::read_to_string(&pid_file).await.unwrap();
    let pids: Vec<i32> = pids.lines().map(|line| line.parse().unwrap()).collect();

    cancellation.cancel();
    let result = tokio::time::timeout(std::time::Duration::from_secs(5), task)
        .await
        .unwrap()
        .unwrap();
    assert!(matches!(
        result,
        Err(openwork_collab::computer::engine::EngineError::Cancelled)
    ));
    for pid in pids {
        tokio::time::timeout(std::time::Duration::from_secs(2), async {
            while unsafe { libc::kill(pid, 0) } == 0 {
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap_or_else(|_| panic!("process {pid} survived cancellation"));
    }
}

#[tokio::test]
async fn inventory_only_checks_executable_presence() {
    let directory = tempfile::tempdir().unwrap();
    let executable = directory.path().join("opencode-inventory");
    tokio::fs::write(
        &executable,
        r#"#!/bin/sh
printf '%s' "$*" > "${0%/*}/unexpected-invocation"
exit 91
"#,
    )
    .await
    .unwrap();
    let mut permissions = tokio::fs::metadata(&executable)
        .await
        .unwrap()
        .permissions();
    permissions.set_mode(0o700);
    tokio::fs::set_permissions(&executable, permissions)
        .await
        .unwrap();

    let inventory = OpenCodeAdapter::with_executable(executable)
        .inventory()
        .await
        .unwrap();
    assert_eq!(inventory.availability, EngineAvailability::Available);
    assert!(!directory.path().join("unexpected-invocation").exists());
}

#[tokio::test]
async fn inventory_reports_a_missing_executable_without_starting_opencode() {
    let directory = tempfile::tempdir().unwrap();
    let inventory = OpenCodeAdapter::with_executable(directory.path().join("not-installed"))
        .inventory()
        .await
        .unwrap();

    assert_eq!(inventory.availability, EngineAvailability::Missing);
}

#[tokio::test]
async fn oversized_jsonl_line_is_stopped_without_unbounded_buffering() {
    let directory = tempfile::tempdir().unwrap();
    let executable = directory.path().join("opencode-too-large");
    tokio::fs::write(
        &executable,
        r#"#!/bin/sh
cat >/dev/null
head -c 1200000 /dev/zero | tr '\000' x
printf '\n'
"#,
    )
    .await
    .unwrap();
    let mut permissions = tokio::fs::metadata(&executable)
        .await
        .unwrap()
        .permissions();
    permissions.set_mode(0o700);
    tokio::fs::set_permissions(&executable, permissions)
        .await
        .unwrap();

    let result = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        OpenCodeAdapter::with_executable(executable).run_turn(TurnRequest {
            home: directory.path().to_path_buf(),
            prompt: "large".to_string(),
            model: None,
            resume_session_id: None,
            environment: Default::default(),
            cancellation: CancellationToken::new(),
        }),
    )
    .await
    .unwrap();
    assert!(matches!(result, Err(EngineError::OutputLimit("stdout"))));
}

#[tokio::test]
async fn process_errors_redact_tokens_and_the_agent_home() {
    let directory = tempfile::tempdir().unwrap();
    let executable = directory.path().join("opencode-secret-error");
    tokio::fs::write(
        &executable,
        r#"#!/bin/sh
cat >/dev/null
printf 'Bearer supersecret token=abc TOKEN=XYZ at %s\n' "$PWD" >&2
exit 1
"#,
    )
    .await
    .unwrap();
    let mut permissions = tokio::fs::metadata(&executable)
        .await
        .unwrap()
        .permissions();
    permissions.set_mode(0o700);
    tokio::fs::set_permissions(&executable, permissions)
        .await
        .unwrap();

    let result = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        OpenCodeAdapter::with_executable(executable).run_turn(TurnRequest {
            home: directory.path().to_path_buf(),
            prompt: "fail safely".to_string(),
            model: None,
            resume_session_id: None,
            environment: Default::default(),
            cancellation: CancellationToken::new(),
        }),
    )
    .await
    .unwrap();
    let error = result.unwrap_err().to_string();

    assert!(error.contains("Bearer <redacted>"));
    assert!(error.contains("token=<redacted>"));
    assert!(error.contains("TOKEN=<redacted>"));
    assert!(error.contains("<agent-home>"));
    assert!(!error.contains("supersecret"));
    assert!(!error.contains("token=abc"));
    assert!(!error.contains("TOKEN=XYZ"));
    assert!(!error.contains(&directory.path().to_string_lossy().into_owned()));
}
