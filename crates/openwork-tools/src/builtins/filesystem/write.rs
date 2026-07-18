use async_trait::async_trait;
use schemars::JsonSchema;
use serde::Deserialize;

use crate::policy::AccessKind;
use crate::{
    TextToolOutput, Tool, ToolCallContext, ToolExecutionError, ToolId, ToolRisk, ToolSessionContext,
};

use super::resolve;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct WriteInput {
    /// Absolute or working-dir-relative path.
    pub path: String,
    /// Full file content to write.
    pub content: String,
}

#[derive(Debug, Default)]
pub struct WriteTool;

#[async_trait]
impl Tool for WriteTool {
    type Input = WriteInput;
    type Output = TextToolOutput;

    fn id(&self) -> ToolId {
        ToolId::new_static("write")
    }

    fn description(&self) -> &'static str {
        "Write text content to a file. Creates the file (and parent directories) if missing; overwrites if it exists."
    }

    fn risk(&self) -> ToolRisk {
        ToolRisk::WorkspaceMutation
    }

    async fn execute(
        &self,
        session: &ToolSessionContext,
        _call: ToolCallContext,
        input: WriteInput,
    ) -> Result<TextToolOutput, ToolExecutionError> {
        let resolved = resolve(&session.working_directory, &input.path);
        session
            .check_path(&resolved, AccessKind::Write)
            .map_err(ToolExecutionError::denied)?;
        if let Some(parent) = resolved.parent() {
            session
                .filesystem
                .create_dir_all(parent)
                .await
                .map_err(|error| {
                    ToolExecutionError::execution(format!("failed to create parent dirs: {error}"))
                })?;
        }
        session
            .filesystem
            .write(&resolved, input.content.as_bytes())
            .await
            .map_err(|error| {
                ToolExecutionError::execution(format!(
                    "failed to write {}: {error}",
                    resolved.display()
                ))
            })?;
        Ok(TextToolOutput::new(format!(
            "wrote {} bytes to {}",
            input.content.len(),
            resolved.display()
        )))
    }
}
