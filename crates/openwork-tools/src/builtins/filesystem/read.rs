use async_trait::async_trait;
use schemars::JsonSchema;
use serde::Deserialize;

use crate::policy::AccessKind;
use crate::{
    TextToolOutput, Tool, ToolCallContext, ToolExecutionError, ToolId, ToolRisk, ToolSessionContext,
};

use super::resolve;

const MAX_BYTES: usize = 1024 * 1024;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ReadInput {
    /// Absolute path, or a path relative to the working directory.
    pub path: String,
}

#[derive(Debug, Default)]
pub struct ReadTool;

#[async_trait]
impl Tool for ReadTool {
    type Input = ReadInput;
    type Output = TextToolOutput;

    fn id(&self) -> ToolId {
        ToolId::new_static("read")
    }

    fn description(&self) -> &'static str {
        "Read a UTF-8 text file from the filesystem. Returns content prefixed with line numbers. Refuses files larger than 1 MB."
    }

    fn risk(&self) -> ToolRisk {
        ToolRisk::ReadOnly
    }

    async fn execute(
        &self,
        session: &ToolSessionContext,
        _call: ToolCallContext,
        input: ReadInput,
    ) -> Result<TextToolOutput, ToolExecutionError> {
        let resolved = resolve(&session.working_directory, &input.path);
        session
            .check_path(&resolved, AccessKind::Read)
            .map_err(ToolExecutionError::denied)?;
        let content = session
            .filesystem
            .read_to_string(&resolved)
            .await
            .map_err(|error| {
                ToolExecutionError::execution(format!(
                    "failed to read {}: {error}",
                    resolved.display()
                ))
            })?;
        if content.len() > MAX_BYTES {
            return Err(ToolExecutionError::execution(format!(
                "file too large: {} bytes (max {})",
                content.len(),
                MAX_BYTES
            )));
        }
        let numbered = content
            .lines()
            .enumerate()
            .map(|(index, line)| format!("{:>6}\t{}", index + 1, line))
            .collect::<Vec<_>>()
            .join("\n");
        Ok(TextToolOutput::new(numbered))
    }
}
