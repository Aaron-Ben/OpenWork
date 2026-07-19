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

use crate::context::PathIntent;

const DEFAULT_MAX_RESULTS: usize = 200;
const MAX_RESULTS: usize = 2000;
const MAX_SEARCH_FILE_BYTES: usize = 1024 * 1024;
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
        if input.max_results == 0 || input.max_results > MAX_RESULTS {
            return Err(ToolExecutionError::invalid_arguments(format!(
                "maxResults must be between 1 and {MAX_RESULTS}"
            )));
        }
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
        let root = session
            .resolve_path(&input.path, AccessKind::Read, PathIntent::MustExist)
            .await?;
        let mut files = session
            .filesystem
            .walk_files(root.as_path())
            .await
            .map_err(|error| ToolExecutionError::execution(format!("grep failed: {error}")))?;
        let mut output = String::new();
        let mut hits = 0usize;

        let mut scanned = 0usize;
        loop {
            let path = tokio::select! {
                _ = call.cancel.cancelled() => {
                    return Err(ToolExecutionError::cancelled("grep cancelled"));
                }
                path = files.next() => path
                    .map_err(|error| ToolExecutionError::execution(format!("grep failed: {error}")))?,
            };
            let Some(path) = path else {
                break;
            };
            scanned += 1;
            if scanned.is_multiple_of(250) {
                call.report_progress(crate::ToolProgress::Message {
                    message: format!("grep scanned {scanned} files"),
                });
            }
            let relative = relative_path(root.as_path(), &path);
            if matcher
                .as_ref()
                .is_some_and(|matcher| !matcher.is_match(&relative))
            {
                continue;
            }
            let content = tokio::select! {
                _ = call.cancel.cancelled() => {
                    return Err(ToolExecutionError::cancelled("grep cancelled"));
                }
                content = session.filesystem.read_to_string_limited(&path, MAX_SEARCH_FILE_BYTES) => content,
            };
            let Ok(content) = content else {
                continue;
            };
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
        } else if hits >= input.max_results {
            output.push_str(&format!(
                "[result limit reached at {}; more matches may exist]\n",
                input.max_results
            ));
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

fn relative_path(root: &std::path::Path, path: &std::path::Path) -> std::path::PathBuf {
    let relative = path.strip_prefix(root).unwrap_or(path);
    if relative.as_os_str().is_empty() {
        path.file_name()
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| path.to_path_buf())
    } else {
        relative.to_path_buf()
    }
}

#[cfg(test)]
mod tests {
    use tokio_util::sync::CancellationToken;

    use super::super::test_support::TestDirectory;
    use super::*;
    use crate::{PermissionProfile, ToolCallId, ToolErrorCode, ToolOutput};

    fn session(workspace: &TestDirectory) -> ToolSessionContext {
        ToolSessionContext::local(
            workspace.path().to_path_buf(),
            PermissionProfile::workspace_write(workspace.path().to_path_buf()),
        )
    }

    #[tokio::test]
    async fn rejects_zero_max_results() {
        let workspace = TestDirectory::new("grep-zero-limit");
        std::fs::write(workspace.path().join("file.txt"), "match").expect("write fixture");

        let error = GrepTool
            .execute(
                &session(&workspace),
                ToolCallContext::new(ToolCallId::new("grep-zero"), CancellationToken::new()),
                GrepInput {
                    pattern: "match".to_string(),
                    path: ".".to_string(),
                    glob: None,
                    output_mode: GrepOutputMode::Content,
                    max_results: 0,
                },
            )
            .await
            .expect_err("zero maxResults");

        assert_eq!(error.code, ToolErrorCode::InvalidArguments);
    }

    #[tokio::test]
    async fn applies_glob_to_paths_relative_to_the_search_root() {
        let workspace = TestDirectory::new("grep-relative-glob");
        std::fs::create_dir(workspace.path().join("src")).expect("create src");
        std::fs::write(workspace.path().join("src/main.rs"), "needle\n").expect("write fixture");

        let result = GrepTool
            .execute(
                &session(&workspace),
                ToolCallContext::new(ToolCallId::new("grep-relative"), CancellationToken::new()),
                GrepInput {
                    pattern: "needle".to_string(),
                    path: ".".to_string(),
                    glob: Some("src/*.rs".to_string()),
                    output_mode: GrepOutputMode::Content,
                    max_results: 10,
                },
            )
            .await
            .expect("grep result")
            .into_tool_result();

        assert!(result.text_content().contains("src/main.rs:1:needle"));
    }

    #[tokio::test]
    async fn reports_when_the_result_limit_stops_the_stream() {
        let workspace = TestDirectory::new("grep-limit");
        std::fs::write(workspace.path().join("file.txt"), "match\nmatch\n").expect("write fixture");

        let result = GrepTool
            .execute(
                &session(&workspace),
                ToolCallContext::new(ToolCallId::new("grep-limit"), CancellationToken::new()),
                GrepInput {
                    pattern: "match".to_string(),
                    path: ".".to_string(),
                    glob: None,
                    output_mode: GrepOutputMode::Content,
                    max_results: 1,
                },
            )
            .await
            .expect("grep result")
            .into_tool_result();

        assert!(result.text_content().contains("result limit reached"));
    }
}
