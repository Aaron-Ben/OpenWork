use std::time::Duration;

use async_trait::async_trait;
use schemars::JsonSchema;
use serde::Deserialize;

use crate::context::PathIntent;
use crate::permission::analyze_bash;
use crate::policy::AccessKind;
use crate::spill::saved_at;
use crate::{
    CapturedOutput, InvocationAnalysis, ProcessRequest, ProcessStatus, Tool, ToolCallContext,
    ToolErrorCode, ToolExecutionError, ToolId, ToolResult, ToolRisk, ToolSessionContext,
};

const DEFAULT_TIMEOUT_MS: u64 = 30_000;
const MAX_TIMEOUT_MS: u64 = 120_000;

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct BashInput {
    /// Shell command to execute.
    pub command: String,
    /// Timeout in milliseconds. Defaults to 30000 and is capped at 120000.
    #[serde(default = "default_timeout_ms")]
    pub timeout_ms: u64,
}

#[derive(Debug, Default)]
pub struct BashTool;

#[async_trait]
impl Tool for BashTool {
    type Input = BashInput;
    type Output = ToolResult;

    fn id(&self) -> ToolId {
        ToolId::new_static("bash")
    }

    fn description(&self) -> &'static str {
        "Run a shell command via `sh -c` in the working directory. Returns stdout and stderr merged in arrival order, and the exit code. Long output keeps its first 2 KB and last 14 KB; the complete output is saved to a file you can read or grep."
    }

    fn risk(&self) -> ToolRisk {
        ToolRisk::ProcessExecution
    }

    fn permission_analysis(
        &self,
        session: &ToolSessionContext,
        input: &Self::Input,
    ) -> InvocationAnalysis {
        analyze_bash(
            &input.command,
            &session.working_directory,
            session.environment.get("PATH").map(String::as_str),
        )
    }

    async fn execute(
        &self,
        session: &ToolSessionContext,
        call: ToolCallContext,
        input: BashInput,
    ) -> Result<ToolResult, ToolExecutionError> {
        let working_directory = session
            .resolve_path(".", AccessKind::Read, PathIntent::MustExist)
            .await?;
        let timeout_ms = input.timeout_ms.clamp(1, MAX_TIMEOUT_MS);
        let output = session
            .process_backend
            .run(
                ProcessRequest {
                    program: "sh".to_string(),
                    arguments: vec!["-c".to_string(), input.command],
                    working_directory: working_directory.as_path().to_path_buf(),
                    environment: session.environment.as_ref().clone(),
                    timeout: Duration::from_millis(timeout_ms),
                    spill_path: session
                        .spill
                        .as_ref()
                        .map(|spill| spill.file_for(&call.call_id)),
                },
                &call,
            )
            .await?;
        let mut combined = render_output(&output.output);
        let footer = match output.status {
            ProcessStatus::Exited { exit_code } => format!(
                "\n[exit {exit_code}; duration {} ms]",
                output.elapsed.as_millis()
            ),
            ProcessStatus::TimedOut => format!(
                "\n[timed out after {timeout_ms} ms; duration {} ms]",
                output.elapsed.as_millis()
            ),
            ProcessStatus::Cancelled => {
                format!("\n[cancelled; duration {} ms]", output.elapsed.as_millis())
            }
        };
        combined.push_str(&footer);

        Ok(match output.status {
            ProcessStatus::Exited { .. } => ToolResult::succeeded(combined),
            ProcessStatus::TimedOut => ToolResult::failed(ToolErrorCode::Timeout, combined, false),
            ProcessStatus::Cancelled => ToolResult::cancelled(combined),
        })
    }
}

fn default_timeout_ms() -> u64 {
    DEFAULT_TIMEOUT_MS
}

/// Head, an omission marker naming the spill file, then the tail
/// (tools.md §9 bash). Build logs put their noise first and their errors
/// last, hence the short head.
fn render_output(output: &CapturedOutput) -> String {
    let mut text = output.head_lossy();
    if output.is_truncated() {
        let saved = match output.spill_path() {
            Some(path) if output.spill_is_capped() => {
                format!(
                    " The first 64 MB of the output is saved at {}.",
                    path.display()
                )
            }
            Some(path) => format!(" {}", saved_at(path)),
            None => String::new(),
        };
        text.push_str(&format!(
            "\n... ({} bytes omitted.{saved})\n",
            output.omitted_bytes()
        ));
    }
    text.push_str(&output.tail_lossy());
    text
}

#[cfg(test)]
mod tests {
    use tokio::sync::mpsc;
    use tokio_util::sync::CancellationToken;

    use super::*;
    use crate::{PermissionProfile, ToolCallId, ToolErrorCode, ToolOutput, ToolProgress};

    fn session() -> ToolSessionContext {
        ToolSessionContext::local(
            std::env::temp_dir(),
            PermissionProfile::from_builtin_rules(std::env::temp_dir()),
        )
    }

    fn call(id: &str) -> ToolCallContext {
        ToolCallContext::new(ToolCallId::new(id), CancellationToken::new())
    }

    /// permissions.md §1.4 / 验收 74: this project enforces no network or
    /// sandbox boundary and must therefore make no claim about one. A
    /// permanently-untrue "not enforced" disclaimer only trains users to
    /// ignore the surrounding text.
    #[tokio::test]
    async fn acc_74_bash_output_makes_no_network_or_sandbox_claim() {
        let result = BashTool
            .execute(
                &session(),
                call("no-network-claim"),
                BashInput {
                    command: "printf hello".to_string(),
                    timeout_ms: 1_000,
                },
            )
            .await
            .expect("printf should complete")
            .into_tool_result();

        let text = result.text_content().to_lowercase();
        for marker in [
            "network",
            "sandbox",
            "restricted",
            "not enforced",
            "isolation",
        ] {
            assert!(
                !text.contains(marker),
                "bash output must not mention {marker:?}: {text}"
            );
        }
    }

    #[tokio::test]
    async fn non_zero_exit_is_a_completed_tool_result() {
        let result = BashTool
            .execute(
                &session(),
                call("non-zero"),
                BashInput {
                    command: "printf failure >&2; exit 7".to_string(),
                    timeout_ms: 1_000,
                },
            )
            .await
            .expect("non-zero exit is still a completed command")
            .into_tool_result();

        assert!(!result.is_error());
        assert!(result.text_content().contains("failure"));
        assert!(result.text_content().contains("exit 7"));
    }

    #[tokio::test]
    async fn timeout_preserves_partial_output() {
        let result = BashTool
            .execute(
                &session(),
                call("timeout-output"),
                BashInput {
                    command: "printf before-timeout; sleep 30".to_string(),
                    timeout_ms: 50,
                },
            )
            .await
            .expect("timeout should produce a terminal tool result")
            .into_tool_result();

        assert_eq!(
            result.error.as_ref().map(|error| error.code),
            Some(ToolErrorCode::Timeout)
        );
        assert!(result.text_content().contains("before-timeout"));
    }

    #[tokio::test]
    async fn cancellation_preserves_partial_output() {
        let cancel = CancellationToken::new();
        let call = ToolCallContext::new(ToolCallId::new("cancel-output"), cancel.clone());
        let task = tokio::spawn(async move {
            BashTool
                .execute(
                    &session(),
                    call,
                    BashInput {
                        command: "printf before-cancel; sleep 30".to_string(),
                        timeout_ms: 60_000,
                    },
                )
                .await
        });
        tokio::time::sleep(Duration::from_millis(50)).await;
        cancel.cancel();

        let result = task
            .await
            .expect("bash task")
            .expect("cancelled tool result")
            .into_tool_result();

        assert_eq!(
            result.error.as_ref().map(|error| error.code),
            Some(ToolErrorCode::Cancelled)
        );
        assert!(result.text_content().contains("before-cancel"));
    }

    #[tokio::test]
    async fn reports_stdout_and_stderr_before_the_final_result() {
        let (progress_tx, mut progress_rx) = mpsc::channel(8);
        let call = call("progress").with_progress_sender(progress_tx);

        BashTool
            .execute(
                &session(),
                call,
                BashInput {
                    command: "printf out; printf err >&2".to_string(),
                    timeout_ms: 1_000,
                },
            )
            .await
            .expect("completed command");

        let mut progress = Vec::new();
        while let Ok(item) = progress_rx.try_recv() {
            progress.push(item);
        }
        assert!(
            progress.iter().any(
                |item| matches!(item, ToolProgress::Stdout { chunk } if chunk.contains("out"))
            )
        );
        assert!(
            progress.iter().any(
                |item| matches!(item, ToolProgress::Stderr { chunk } if chunk.contains("err"))
            )
        );
    }

    /// tools.md §12 #25 and #26: long output shows its first 2 KB and last
    /// 14 KB, names the omitted byte count, and saves the complete output.
    #[cfg(unix)]
    #[tokio::test]
    async fn acc_25_long_output_keeps_head_and_tail_and_spills_the_rest() {
        let spill_root =
            std::env::temp_dir().join(format!("openwork-bash-spill-{}", std::process::id()));
        let session = session().with_spill_directory(crate::SpillDirectory::new(&spill_root));
        let result = BashTool
            .execute(
                &session,
                call("long-output"),
                BashInput {
                    command: "seq 1 20000".to_string(),
                    timeout_ms: 5_000,
                },
            )
            .await
            .expect("seq completes")
            .into_tool_result();

        let text = result.text_content();
        let saved = spill_root.join("long-output.txt");
        let total = (1..=20000).map(|n| format!("{n}\n")).collect::<String>();
        let omitted = total.len() - 16 * 1024;
        assert!(text.starts_with("1\n2\n3\n"));
        assert!(text.contains(&format!(
            "... ({omitted} bytes omitted. Full output saved at {} — use read with offset/limit, or grep, to look at it.)",
            saved.display()
        )));
        assert!(text.contains("19999\n20000\n\n[exit 0;"));
        assert!(text.len() < 16 * 1024 + 512);
        assert_eq!(std::fs::read_to_string(&saved).expect("spill file"), total);
        let _ = std::fs::remove_dir_all(spill_root);
    }
}
