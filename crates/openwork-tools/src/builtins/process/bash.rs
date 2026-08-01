use std::time::Duration;

use async_trait::async_trait;
use schemars::JsonSchema;
use serde::Deserialize;

use crate::builtins::truncate_output;
use crate::context::PathIntent;
use crate::permission::analyze_bash;
use crate::policy::AccessKind;
use crate::{
    InvocationAnalysis, ProcessRequest, ProcessStatus, Tool, ToolCallContext, ToolErrorCode,
    ToolExecutionError, ToolId, ToolResult, ToolRisk, ToolSessionContext,
};

const MAX_OUTPUT_BYTES: usize = 32 * 1024;
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
        "Run a shell command via `sh -c` in the working directory. Returns combined stdout/stderr and the exit code. Subject to approval."
    }

    fn risk(&self) -> ToolRisk {
        ToolRisk::ProcessExecution
    }

    fn permission_analysis(
        &self,
        session: &ToolSessionContext,
        input: &Self::Input,
    ) -> InvocationAnalysis {
        analyze_bash(&input.command, &session.working_directory)
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
                },
                &call,
            )
            .await?;
        let stdout = output.stdout.render_lossy();
        let stderr = output.stderr.render_lossy();
        let mut combined = String::new();
        if !stdout.is_empty() {
            combined.push_str(&stdout);
        }
        if !stderr.is_empty() {
            if !combined.is_empty() {
                combined.push('\n');
            }
            combined.push_str("[stderr]\n");
            combined.push_str(&stderr);
        }
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
        let mut combined = truncate_output(combined, MAX_OUTPUT_BYTES.saturating_sub(footer.len()));
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
}
