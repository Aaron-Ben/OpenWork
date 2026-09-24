#![cfg(unix)]

mod support;

use std::os::unix::fs::PermissionsExt;
use std::time::Duration;

use openwork_collab::computer::engine::{
    AgentEngineRuntime, ClassifyRequest, EngineAdapter, EngineAvailability, EngineError,
    EngineRuntimeConfig, TurnRequest, TurnResult,
};
use openwork_collab::computer::opencode::OpenCodeAdapter;
use openwork_sandbox::{EngineConfinement, SANDBOX_EXEC, SandboxEnvironment, Seatbelt};
use tokio_util::sync::CancellationToken;

#[tokio::test]
async fn opencode_run_turn_uses_stdin_and_returns_resumable_structured_result() {
    let directory = tempfile::tempdir().unwrap();
    let executable = support::fake_opencode(&directory).await;
    let mut runtime = runtime(adapter(executable, &directory), &directory, "openai/gpt-5").await;

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
async fn main_turn_has_no_default_silence_or_wall_clock_timeout() {
    let directory = tempfile::tempdir().unwrap();
    let executable = directory.path().join("opencode-silent-work");
    tokio::fs::write(
        &executable,
        r#"#!/bin/sh
cat >/dev/null
sleep 1
printf '%s\n' \
  '{"type":"step_start","sessionID":"ses_silent"}' \
  '{"type":"text","part":{"text":"finished after silence"}}'
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

    let mut runtime = runtime_with_timeout(
        adapter(executable, &directory),
        &directory,
        "test/model",
        None,
    )
    .await;
    let result = tokio::time::timeout(
        Duration::from_secs(5),
        runtime.run_turn(TurnRequest {
            prompt: "do quiet work".to_string(),
            cancellation: CancellationToken::new(),
        }),
    )
    .await
    .expect("a silent turn should finish without a default timeout")
    .unwrap();

    assert_eq!(result.text, "finished after silence");
}

#[tokio::test]
async fn an_explicit_main_turn_timeout_remains_available() {
    let directory = tempfile::tempdir().unwrap();
    let executable = directory.path().join("opencode-explicit-timeout");
    tokio::fs::write(
        &executable,
        r#"#!/bin/sh
cat >/dev/null
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

    let mut runtime = runtime_with_timeout(
        adapter(executable, &directory),
        &directory,
        "test/model",
        Some(Duration::from_millis(50)),
    )
    .await;
    let result = tokio::time::timeout(
        Duration::from_secs(5),
        runtime.run_turn(TurnRequest {
            prompt: "bound this turn".to_string(),
            cancellation: CancellationToken::new(),
        }),
    )
    .await
    .expect("the configured timeout should stop the process");

    assert!(matches!(
        result,
        Err(EngineError::Timeout { operation: "turn" })
    ));
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

    let mut runtime = runtime(adapter(executable, &directory), &directory, "test/model").await;
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

    let mut runtime = runtime(adapter(executable, &directory), &directory, "test/model").await;
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

    let mut runtime = runtime(adapter(executable, &directory), &directory, "test/model").await;
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
    let adapter = adapter(executable, &directory);

    let result = adapter
        .classify(ClassifyRequest {
            cwd: directory.path().to_path_buf(),
            config_root: directory.path().join("config"),
            confinement: confinement(&directory),
            prompt: r#"{"actionable":false}"#.to_string(),
            model: None,
            environment: Default::default(),
            cancellation: CancellationToken::new(),
        })
        .await
        .unwrap();

    assert_eq!(result.text, r#"{"actionable":false}"#);
}

/// 设置了 `OPENCODE_DISABLE_PROJECT_CONFIG` 后 OpenCode 不再读 cwd 上方的 AGENTS.md，
/// 协作契约只能经全局配置的 `instructions` 绝对路径进入系统提示词。
#[tokio::test]
async fn main_turn_config_loads_the_managed_agents_file_as_instructions() {
    let directory = tempfile::tempdir().unwrap();
    let executable = config_echoing_opencode(&directory).await;
    let mut runtime = runtime(adapter(executable, &directory), &directory, "test/model").await;

    let result = runtime
        .run_turn(TurnRequest {
            prompt: "work".to_string(),
            cancellation: CancellationToken::new(),
        })
        .await
        .unwrap();

    let config: serde_json::Value = serde_json::from_str(&result.text).unwrap();
    assert_eq!(
        config,
        serde_json::json!({
            "permission": {"*": "allow"},
            "instructions": [directory.path().join("AGENTS.md").to_string_lossy()],
            "provider": {"test": {"models": {"model": {"status": "active"}}}},
        })
    );
}

/// collaboration.md §6：OpenCode 刷新模型目录后会删掉标为 deprecated 的模型（opencode
/// `provider/provider.ts` 1694 行），用户选的模型因此在第一次运行后失效。派生配置显式把
/// Agent 选的主模型和判断模型标为 active；模型 id 里的 `/` 属于模型名。
#[tokio::test]
async fn derived_configs_keep_the_chosen_models_active_even_when_the_catalog_deprecates_them() {
    let directory = tempfile::tempdir().unwrap();
    let executable = config_echoing_opencode(&directory).await;
    let adapter = adapter(executable, &directory);

    let result = adapter
        .classify(ClassifyRequest {
            cwd: directory.path().to_path_buf(),
            config_root: directory.path().join("config"),
            confinement: confinement(&directory),
            prompt: "classify".to_string(),
            model: Some("openrouter/deepseek/deepseek-v4-flash".to_string()),
            environment: Default::default(),
            cancellation: CancellationToken::new(),
        })
        .await
        .unwrap();

    let config: serde_json::Value = serde_json::from_str(&result.text).unwrap();
    assert_eq!(
        config,
        serde_json::json!({
            "permission": {"*": "allow"},
            "provider": {"openrouter": {"models": {"deepseek/deepseek-v4-flash": {"status": "active"}}}},
        })
    );
}

/// 小模型分类不带 Agent persona：它的配置目录与主 Turn 分开，
/// 否则 OpenCode 会把两份配置里的 `instructions` 拼在一起。
#[tokio::test]
async fn classify_config_does_not_load_agent_instructions() {
    let directory = tempfile::tempdir().unwrap();
    let executable = config_echoing_opencode(&directory).await;
    let adapter = adapter(executable, &directory);
    let mut runtime = runtime(adapter.clone(), &directory, "test/model").await;
    runtime
        .run_turn(TurnRequest {
            prompt: "work".to_string(),
            cancellation: CancellationToken::new(),
        })
        .await
        .unwrap();

    let result = adapter
        .classify(ClassifyRequest {
            cwd: directory.path().to_path_buf(),
            config_root: directory.path().join("config"),
            confinement: confinement(&directory),
            prompt: "classify".to_string(),
            model: None,
            environment: Default::default(),
            cancellation: CancellationToken::new(),
        })
        .await
        .unwrap();

    let config: serde_json::Value = serde_json::from_str(&result.text).unwrap();
    assert_eq!(config, serde_json::json!({"permission": {"*": "allow"}}));
}

/// 真实 OpenCode 找不到 `--session` 时 stdout 为空，只在 stderr 写 `Session not found`
/// 并以 1 退出（opencode `cli/cmd/run.ts`）。adapter 必须清掉旧 session 并用新会话重跑，
/// 否则每次轮询都会带着同一个失效的 id 失败。
#[tokio::test]
async fn a_stale_session_reported_only_on_stderr_starts_a_fresh_session() {
    let directory = tempfile::tempdir().unwrap();
    let executable = executable_script(
        &directory,
        "opencode-stale-session",
        r#"#!/bin/sh
cat >/dev/null
case " $* " in
  *" --session "*) echo 'Error: Session not found' >&2; exit 1 ;;
esac
printf '%s\n' \
  '{"type":"step_start","sessionID":"ses_fresh"}' \
  '{"type":"text","part":{"text":"fresh"}}'
"#,
    )
    .await;
    tokio::fs::write(
        directory.path().join("session.json"),
        r#"{"engine_id":"opencode","model":"test/model","context_fingerprint":"test-persona","session_id":"ses_stale","updated_at":"2026-09-24T00:00:00Z"}"#,
    )
    .await
    .unwrap();
    let mut runtime = runtime(adapter(executable, &directory), &directory, "test/model").await;

    let result = turn(&mut runtime).await.unwrap();

    assert_eq!(result.text, "fresh");
    let session: serde_json::Value = serde_json::from_slice(
        &tokio::fs::read(directory.path().join("session.json"))
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(session["session_id"], "ses_fresh");
}

/// collaboration.md §3.1：沙箱自检失败时 inventory 报错，Turn 也不会在沙箱外启动 OpenCode。
#[tokio::test]
async fn unavailable_sandbox_fails_closed_without_starting_opencode() {
    let directory = tempfile::tempdir().unwrap();
    let executable = executable_script(
        &directory,
        "opencode-marker",
        &format!(
            "#!/bin/sh\ntouch '{}'\n",
            directory.path().join("started").display()
        ),
    )
    .await;
    let adapter = OpenCodeAdapter::new(
        executable,
        Seatbelt::probe(directory.path().join("missing-sandbox-exec")),
        directory.path().join("user-data"),
    );

    assert!(matches!(
        adapter.probe().await,
        Err(EngineError::Sandbox(_))
    ));
    let mut runtime = runtime(adapter, &directory, "test/model").await;
    let result = runtime
        .run_turn(TurnRequest {
            prompt: "work".to_string(),
            cancellation: CancellationToken::new(),
        })
        .await;

    assert!(matches!(result, Err(EngineError::Sandbox(_))));
    assert!(!directory.path().join("started").exists());
}

/// collaboration.md §3.1：沙箱读不到用户的 OpenCode 数据目录，登录信息由 Computer 读出后
/// 经 `OPENCODE_AUTH_CONTENT` 传入；用户没有登录过时不设置这个变量。
#[tokio::test]
async fn user_auth_reaches_opencode_through_the_environment() {
    let directory = tempfile::tempdir().unwrap();
    let executable = executable_script(
        &directory,
        "opencode-auth-echo",
        r#"#!/bin/sh
cat >/dev/null
text=$(printf '%s' "${OPENCODE_AUTH_CONTENT-unset}" | sed 's/\\/\\\\/g; s/"/\\"/g')
printf '%s\n' \
  '{"type":"step_start","sessionID":"ses_auth"}' \
  "{\"type\":\"text\",\"part\":{\"text\":\"$text\"}}"
"#,
    )
    .await;
    let adapter = adapter(executable, &directory);
    let mut runtime = runtime(adapter, &directory, "test/model").await;
    assert_eq!(turn(&mut runtime).await.unwrap().text, "unset");
    let auth = directory.path().join("user-data/opencode/auth.json");
    tokio::fs::create_dir_all(auth.parent().unwrap())
        .await
        .unwrap();
    tokio::fs::write(&auth, r#"{"deepseek":{"type":"api","key":"sk-test"}}"#)
        .await
        .unwrap();
    assert_eq!(
        turn(&mut runtime).await.unwrap().text,
        r#"{"deepseek":{"type":"api","key":"sk-test"}}"#
    );
}

/// 登录信息进入环境变量，超过 64 KiB 时拒绝启动而不是截断。
#[tokio::test]
async fn oversized_user_auth_is_rejected_before_opencode_starts() {
    let directory = tempfile::tempdir().unwrap();
    let executable = executable_script(
        &directory,
        "opencode-marker",
        &format!(
            "#!/bin/sh\ntouch '{}'\n",
            directory.path().join("started").display()
        ),
    )
    .await;
    let auth = directory.path().join("user-data/opencode/auth.json");
    tokio::fs::create_dir_all(auth.parent().unwrap())
        .await
        .unwrap();
    tokio::fs::write(&auth, vec![b'x'; 64 * 1024 + 1])
        .await
        .unwrap();
    let mut runtime = runtime(adapter(executable, &directory), &directory, "test/model").await;

    let result = runtime
        .run_turn(TurnRequest {
            prompt: "work".to_string(),
            cancellation: CancellationToken::new(),
        })
        .await;

    assert!(matches!(
        result,
        Err(EngineError::Io(error)) if error.kind() == std::io::ErrorKind::InvalidData
    ));
    assert!(!directory.path().join("started").exists());
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
    let mut runtime = runtime(adapter(executable, &directory), &directory, "test/model").await;
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

    let inventory = adapter(executable, &directory).probe().await.unwrap();
    assert_eq!(inventory.availability, EngineAvailability::Available);
    assert!(!directory.path().join("unexpected-invocation").exists());
}

#[tokio::test]
async fn inventory_reports_a_missing_executable_without_starting_opencode() {
    let directory = tempfile::tempdir().unwrap();
    let inventory = adapter(directory.path().join("not-installed"), &directory)
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

    let mut runtime = runtime(adapter(executable, &directory), &directory, "test/model").await;
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

    let mut runtime = runtime(adapter(executable, &directory), &directory, "test/model").await;
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

/// 把 OpenCode 实际读到的全局 `opencode.json` 原样作为回复文本输出。
async fn config_echoing_opencode(directory: &tempfile::TempDir) -> std::path::PathBuf {
    let executable = directory.path().join("opencode-config-echo");
    tokio::fs::write(
        &executable,
        r#"#!/bin/sh
cat >/dev/null
config=$(cat "$XDG_CONFIG_HOME/opencode/opencode.json")
text=$(printf '%s' "$config" | sed 's/\\/\\\\/g; s/"/\\"/g')
printf '%s\n' \
  '{"type":"step_start","sessionID":"ses_config"}' \
  "{\"type\":\"text\",\"part\":{\"text\":\"$text\"}}"
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

async fn turn(runtime: &mut Box<dyn AgentEngineRuntime>) -> Result<TurnResult, EngineError> {
    runtime
        .run_turn(TurnRequest {
            prompt: "work".to_string(),
            cancellation: CancellationToken::new(),
        })
        .await
}

async fn executable_script(
    directory: &tempfile::TempDir,
    name: &str,
    script: &str,
) -> std::path::PathBuf {
    let executable = directory.path().join(name);
    tokio::fs::write(&executable, script).await.unwrap();
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

/// 在真实 Seatbelt 下运行；用户数据目录是测试目录里的 `user-data`。
fn adapter(
    executable: impl Into<std::path::PathBuf>,
    directory: &tempfile::TempDir,
) -> OpenCodeAdapter {
    OpenCodeAdapter::new(
        executable,
        Seatbelt::probe(SANDBOX_EXEC),
        directory.path().join("user-data"),
    )
}

/// 测试目录建在系统临时根下，本来就可写；这里只是给出一个真实的围栏。
fn confinement(directory: &tempfile::TempDir) -> EngineConfinement {
    EngineConfinement::new(&SandboxEnvironment::detect(Vec::new()).unwrap())
        .with_writable_root(directory.path())
}

async fn runtime(
    adapter: OpenCodeAdapter,
    directory: &tempfile::TempDir,
    model: &str,
) -> Box<dyn AgentEngineRuntime> {
    runtime_with_timeout(adapter, directory, model, None).await
}

async fn runtime_with_timeout(
    adapter: OpenCodeAdapter,
    directory: &tempfile::TempDir,
    model: &str,
    turn_timeout: Option<Duration>,
) -> Box<dyn AgentEngineRuntime> {
    adapter
        .create_agent_runtime(EngineRuntimeConfig {
            home: directory.path().to_path_buf(),
            config_root: directory.path().join("config"),
            instructions_file: directory.path().join("AGENTS.md"),
            confinement: confinement(directory),
            state_file: directory.path().join("session.json"),
            context_fingerprint: "test-persona".to_string(),
            model: model.to_string(),
            environment: Default::default(),
            turn_timeout,
        })
        .await
        .unwrap()
}
