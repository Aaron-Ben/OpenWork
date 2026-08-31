#![cfg(unix)]

mod support;

use std::os::unix::fs::PermissionsExt;

use openwork_collab::computer::engine::{
    AgentEngineRuntime, ClassifyRequest, EngineAdapter, EngineAvailability, EngineError,
    EngineRuntimeConfig, TurnRequest,
};
use openwork_collab::computer::opencode::OpenCodeAdapter;
use tokio_util::sync::CancellationToken;

#[tokio::test]
async fn opencode_run_turn_uses_stdin_and_returns_resumable_structured_result() {
    let directory = tempfile::tempdir().unwrap();
    let executable = support::fake_opencode(&directory).await;
    let mut runtime = runtime(
        OpenCodeAdapter::with_executable(executable),
        &directory,
        "openai/gpt-5",
    )
    .await;

    let result = runtime
        .run_turn(TurnRequest {
            prompt: "continue the work".to_string(),
            cancellation: CancellationToken::new(),
        })
        .await
        .unwrap();

    assert_eq!(result.text, "done");
    let session: serde_json::Value = serde_json::from_slice(
        &tokio::fs::read(directory.path().join("session.json"))
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(session["session_id"], "ses_local");
    assert_eq!(session["context_fingerprint"], "test-persona");
    assert!(session.get("persona_hash").is_none());
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

    let mut runtime = runtime(
        OpenCodeAdapter::with_executable(executable),
        &directory,
        "test/model",
    )
    .await;
    let result = runtime
        .run_turn(TurnRequest {
            prompt: "reply".to_string(),
            cancellation: CancellationToken::new(),
        })
        .await;

    assert!(matches!(
        result,
        Err(EngineError::Reported { detail }) if detail == "Insufficient balance"
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

    let mut runtime = runtime(
        OpenCodeAdapter::with_executable(executable),
        &directory,
        "test/model",
    )
    .await;
    let result = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        runtime.run_turn(TurnRequest {
            prompt: "reply".to_string(),
            cancellation: CancellationToken::new(),
        }),
    )
    .await
    .expect("reported errors must not wait for the no-output timeout");

    assert!(matches!(
        result,
        Err(EngineError::RateLimited { detail, .. }) if detail.contains("Rate limit exceeded")
    ));
}

#[tokio::test]
async fn stderr_rate_limit_is_mapped_inside_the_opencode_adapter() {
    let directory = tempfile::tempdir().unwrap();
    let executable = directory.path().join("opencode-stderr-rate-limit");
    tokio::fs::write(
        &executable,
        r#"#!/bin/sh
cat >/dev/null
echo 'provider returned HTTP 429: Too Many Requests' >&2
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

    let mut runtime = runtime(
        OpenCodeAdapter::with_executable(executable),
        &directory,
        "test/model",
    )
    .await;
    let result = runtime
        .run_turn(TurnRequest {
            prompt: "reply".to_string(),
            cancellation: CancellationToken::new(),
        })
        .await;

    assert!(matches!(
        result,
        Err(EngineError::RateLimited { detail, .. })
            if detail.contains("429") && detail.contains("Too Many Requests")
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
            config_root: directory.path().join("config"),
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
    let mut runtime = runtime(
        OpenCodeAdapter::with_executable(executable),
        &directory,
        "test/model",
    )
    .await;
    let cancellation = CancellationToken::new();
    let pid_file = directory.path().join("pids");
    let pid_file_for_task = pid_file.clone();
    let task_cancellation = cancellation.clone();
    let task = tokio::spawn(async move {
        runtime
            .run_turn(TurnRequest {
                prompt: pid_file_for_task.to_string_lossy().into_owned(),
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
        .probe()
        .await
        .unwrap();
    assert_eq!(inventory.availability, EngineAvailability::Available);
    assert!(!directory.path().join("unexpected-invocation").exists());
}

#[tokio::test]
async fn inventory_reports_a_missing_executable_without_starting_opencode() {
    let directory = tempfile::tempdir().unwrap();
    let inventory = OpenCodeAdapter::with_executable(directory.path().join("not-installed"))
        .probe()
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

    let mut runtime = runtime(
        OpenCodeAdapter::with_executable(executable),
        &directory,
        "test/model",
    )
    .await;
    let result = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        runtime.run_turn(TurnRequest {
            prompt: "large".to_string(),
            cancellation: CancellationToken::new(),
        }),
    )
    .await
    .unwrap();
    assert!(matches!(
        result,
        Err(EngineError::OutputLimit {
            stream: "stdout",
            ..
        })
    ));
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

    let mut runtime = runtime(
        OpenCodeAdapter::with_executable(executable),
        &directory,
        "test/model",
    )
    .await;
    let result = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        runtime.run_turn(TurnRequest {
            prompt: "fail safely".to_string(),
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

async fn runtime(
    adapter: OpenCodeAdapter,
    directory: &tempfile::TempDir,
    model: &str,
) -> Box<dyn AgentEngineRuntime> {
    adapter
        .create_agent_runtime(EngineRuntimeConfig {
            home: directory.path().to_path_buf(),
            config_root: directory.path().join("config"),
            state_file: directory.path().join("session.json"),
            context_fingerprint: "test-persona".to_string(),
            model: model.to_string(),
            environment: Default::default(),
        })
        .await
        .unwrap()
}
