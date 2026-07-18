use async_trait::async_trait;
use globset::{Glob as GlobPattern, GlobSet};
use schemars::JsonSchema;
use serde::Deserialize;

use crate::builtins::truncate_output;
use crate::policy::AccessKind;
use crate::{
    TextToolOutput, Tool, ToolCallContext, ToolExecutionError, ToolId, ToolRisk, ToolSessionContext,
};

use super::resolve;

const MAX_RESULTS: usize = 2000;
const MAX_OUTPUT_BYTES: usize = 32 * 1024;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct GlobInput {
    /// Glob pattern, for example `**/*.rs` or `src/**/*.ts`.
    pub pattern: String,
    /// Directory to search; defaults to the working directory.
    #[serde(default = "default_path")]
    pub path: String,
}

#[derive(Debug, Default)]
pub struct GlobTool;

#[async_trait]
impl Tool for GlobTool {
    type Input = GlobInput;
    type Output = TextToolOutput;

    fn id(&self) -> ToolId {
        ToolId::new_static("glob")
    }

    fn description(&self) -> &'static str {
        "Find files by name pattern (e.g. \"**/*.rs\"). Respects .gitignore. Returns matching file paths, one per line."
    }

    fn risk(&self) -> ToolRisk {
        ToolRisk::ReadOnly
    }

    async fn execute(
        &self,
        session: &ToolSessionContext,
        _call: ToolCallContext,
        input: GlobInput,
    ) -> Result<TextToolOutput, ToolExecutionError> {
        let glob = GlobPattern::new(&input.pattern).map_err(|error| {
            ToolExecutionError::invalid_arguments(format!("invalid glob: {error}"))
        })?;
        let set = GlobSet::builder().add(glob).build().map_err(|error| {
            ToolExecutionError::invalid_arguments(format!("invalid glob: {error}"))
        })?;
        let root = resolve(&session.working_directory, &input.path);
        session
            .check_path(&root, AccessKind::Read)
            .map_err(ToolExecutionError::denied)?;
        if !session
            .filesystem
            .exists(&root)
            .await
            .map_err(|error| ToolExecutionError::execution(error.to_string()))?
        {
            return Ok(TextToolOutput::new(format!(
                "path not found: {}",
                root.display()
            )));
        }
        let files = session
            .filesystem
            .walk_files(&root)
            .await
            .map_err(|error| ToolExecutionError::execution(format!("glob failed: {error}")))?;
        let mut matches = files
            .into_iter()
            .filter(|path| set.is_match(path))
            .take(MAX_RESULTS)
            .map(|path| {
                path.strip_prefix(&root)
                    .unwrap_or(&path)
                    .display()
                    .to_string()
            })
            .collect::<Vec<_>>();
        matches.sort();
        let output = if matches.is_empty() {
            "no files matched\n".to_string()
        } else {
            format!("{}\n", matches.join("\n"))
        };
        Ok(TextToolOutput::new(truncate_output(
            output,
            MAX_OUTPUT_BYTES,
        )))
    }
}

fn default_path() -> String {
    ".".to_string()
}
