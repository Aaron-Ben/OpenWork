use async_trait::async_trait;
use schemars::JsonSchema;
use serde::Deserialize;

use crate::policy::AccessKind;
use crate::{
    TextToolOutput, Tool, ToolCallContext, ToolExecutionError, ToolId, ToolRisk, ToolSessionContext,
};

use crate::context::PathIntent;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ListInput {
    /// Directory path; defaults to the working directory.
    #[serde(default = "default_path")]
    pub path: String,
}

#[derive(Debug, Default)]
pub struct ListTool;

#[async_trait]
impl Tool for ListTool {
    type Input = ListInput;
    type Output = TextToolOutput;

    fn id(&self) -> ToolId {
        ToolId::new_static("list")
    }

    fn description(&self) -> &'static str {
        "List entries in a directory. Directories are suffixed with '/'. Defaults to the working directory."
    }

    fn risk(&self) -> ToolRisk {
        ToolRisk::ReadOnly
    }

    async fn execute(
        &self,
        session: &ToolSessionContext,
        _call: ToolCallContext,
        input: ListInput,
    ) -> Result<TextToolOutput, ToolExecutionError> {
        let resolved = session
            .resolve_path(&input.path, AccessKind::Read, PathIntent::MustExist)
            .await?;
        let mut names = session
            .filesystem
            .read_dir(resolved.as_path())
            .await
            .map_err(|error| {
                ToolExecutionError::execution(format!(
                    "failed to list {}: {error}",
                    resolved.as_path().display()
                ))
            })?
            .into_iter()
            .map(|entry| {
                if entry.is_directory {
                    format!("{}/", entry.name)
                } else {
                    entry.name
                }
            })
            .collect::<Vec<_>>();
        names.sort();
        let output = if names.is_empty() {
            format!("{} is empty", resolved.as_path().display())
        } else {
            names.join("\n")
        };
        Ok(TextToolOutput::new(output))
    }
}

fn default_path() -> String {
    ".".to_string()
}
