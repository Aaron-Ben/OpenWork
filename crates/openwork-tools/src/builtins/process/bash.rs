use std::time::Duration;

use async_trait::async_trait;
use schemars::JsonSchema;
use serde::Deserialize;

use crate::builtins::truncate_output;
use crate::policy::{AccessKind, NetworkMode};
use crate::{
    ProcessRequest, TextToolOutput, Tool, ToolCallContext, ToolExecutionError, ToolId, ToolRisk,
    ToolSessionContext,
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
    type Output = TextToolOutput;

    fn id(&self) -> ToolId {
        ToolId::new_static("bash")
    }

    fn description(&self) -> &'static str {
        "Run a shell command via `sh -c` in the working directory. Returns combined stdout/stderr and the exit code. Subject to approval."
    }

    fn risk(&self) -> ToolRisk {
        ToolRisk::ProcessExecution
    }

    async fn execute(
        &self,
        session: &ToolSessionContext,
        call: ToolCallContext,
        input: BashInput,
    ) -> Result<TextToolOutput, ToolExecutionError> {
        session
            .check_path(&session.working_directory, AccessKind::Read)
            .map_err(ToolExecutionError::denied)?;
        let mut environment = session.environment.as_ref().clone();
        if session.permissions.network == NetworkMode::Restricted {
            environment.insert("OPENWORK_NETWORK_RESTRICTED".to_string(), "1".to_string());
        }
        let timeout_ms = input.timeout_ms.clamp(1, MAX_TIMEOUT_MS);
        let output = session
            .process_backend
            .run(
                ProcessRequest {
                    program: "sh".to_string(),
                    arguments: vec!["-c".to_string(), input.command],
                    working_directory: session.working_directory.clone(),
                    environment,
                    timeout: Duration::from_millis(timeout_ms),
                },
                &call,
            )
            .await?;
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
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
        let mut combined = truncate_output(combined, MAX_OUTPUT_BYTES);
        combined.push_str(&format!(
            "\n[exit {}; duration {} ms]",
            output.exit_code,
            output.elapsed.as_millis()
        ));

        if output.exit_code == 0 {
            Ok(TextToolOutput::new(combined))
        } else {
            Err(ToolExecutionError::execution(combined))
        }
    }
}

fn default_timeout_ms() -> u64 {
    DEFAULT_TIMEOUT_MS
}
