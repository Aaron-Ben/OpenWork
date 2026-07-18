use async_trait::async_trait;
use globset::Glob as GlobSpec;
use regex::Regex;
use schemars::JsonSchema;
use serde::Deserialize;

use crate::builtins::truncate_output;
use crate::policy::AccessKind;
use crate::{
    TextToolOutput, Tool, ToolCallContext, ToolExecutionError, ToolId, ToolRisk, ToolSessionContext,
};

use super::resolve;

const DEFAULT_MAX_RESULTS: usize = 200;
const MAX_OUTPUT_BYTES: usize = 32 * 1024;

#[derive(Debug, Clone, Copy, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
#[schemars(inline)]
pub enum GrepOutputMode {
    #[default]
    Content,
    FilesWithMatches,
    Count,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct GrepInput {
    /// Regular expression to search for.
    pub pattern: String,
    /// Directory or file to search; defaults to the working directory.
    #[serde(default = "default_path")]
    pub path: String,
    /// Optional glob to filter files, for example `*.rs`.
    #[serde(default)]
    pub glob: Option<String>,
    /// Output shape; defaults to `content`.
    #[serde(default)]
    pub output_mode: GrepOutputMode,
    /// Maximum number of results; defaults to 200.
    #[serde(default = "default_max_results")]
    pub max_results: usize,
}

#[derive(Debug, Default)]
pub struct GrepTool;

#[async_trait]
impl Tool for GrepTool {
    type Input = GrepInput;
    type Output = TextToolOutput;

    fn id(&self) -> ToolId {
        ToolId::new_static("grep")
    }

    fn description(&self) -> &'static str {
        "Search file contents with a regular expression (ripgrep-like; respects .gitignore). Returns `path:line:content` by default, just file paths in `files_with_matches` mode, or `path:count` in `count` mode. Use `glob` to filter file types (e.g. \"*.rs\")."
    }

    fn risk(&self) -> ToolRisk {
        ToolRisk::ReadOnly
    }

    async fn execute(
        &self,
        session: &ToolSessionContext,
        call: ToolCallContext,
        input: GrepInput,
    ) -> Result<TextToolOutput, ToolExecutionError> {
        let regex = Regex::new(&input.pattern).map_err(|error| {
            ToolExecutionError::invalid_arguments(format!("invalid regex: {error}"))
        })?;
        let matcher = input
            .glob
            .as_deref()
            .map(GlobSpec::new)
            .transpose()
            .map_err(|error| {
                ToolExecutionError::invalid_arguments(format!("invalid glob: {error}"))
            })?
            .map(|glob| glob.compile_matcher());
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
            .map_err(|error| ToolExecutionError::execution(format!("grep failed: {error}")))?;
        let mut output = String::new();
        let mut hits = 0usize;

        for path in files {
            if call.cancel.is_cancelled() {
                return Err(ToolExecutionError::cancelled("grep cancelled"));
            }
            if matcher
                .as_ref()
                .is_some_and(|matcher| !matcher.is_match(&path))
            {
                continue;
            }
            let Ok(content) = session.filesystem.read_to_string(&path).await else {
                continue;
            };
            let relative = path.strip_prefix(&root).unwrap_or(&path);
            match input.output_mode {
                GrepOutputMode::FilesWithMatches => {
                    if content.lines().any(|line| regex.is_match(line)) {
                        output.push_str(&format!("{}\n", relative.display()));
                        hits += 1;
                    }
                }
                GrepOutputMode::Count => {
                    let count = content.lines().filter(|line| regex.is_match(line)).count();
                    if count > 0 {
                        output.push_str(&format!("{}:{}\n", relative.display(), count));
                        hits += 1;
                    }
                }
                GrepOutputMode::Content => {
                    for (index, line) in content.lines().enumerate() {
                        if regex.is_match(line) {
                            output.push_str(&format!(
                                "{}:{}:{}\n",
                                relative.display(),
                                index + 1,
                                line
                            ));
                            hits += 1;
                            if hits >= input.max_results {
                                break;
                            }
                        }
                    }
                }
            }
            if hits >= input.max_results {
                break;
            }
        }

        if output.is_empty() {
            output = format!("no matches for /{}/", regex.as_str());
        }
        Ok(TextToolOutput::new(truncate_output(
            output,
            MAX_OUTPUT_BYTES,
        )))
    }
}

fn default_path() -> String {
    ".".to_string()
}

fn default_max_results() -> usize {
    DEFAULT_MAX_RESULTS
}
