use async_trait::async_trait;
use globset::{Glob as GlobPattern, GlobSet};
use schemars::JsonSchema;
use serde::Deserialize;

use crate::builtins::truncate_output;
use crate::policy::AccessKind;
use crate::{
    AnalysisUnit, Effect, InvocationAnalysis, TextToolOutput, Tool, ToolCallContext,
    ToolExecutionError, ToolId, ToolRisk, ToolSessionContext,
};

use crate::context::PathIntent;

const DEFAULT_MAX_RESULTS: usize = 200;
const MAX_RESULTS: usize = 2000;
const MAX_OUTPUT_BYTES: usize = 32 * 1024;

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct GlobInput {
    /// Glob pattern, for example `**/*.rs` or `src/**/*.ts`.
    pub pattern: String,
    /// Directory to search; defaults to the working directory.
    #[serde(default = "default_path")]
    pub path: String,
    /// Maximum number of matching paths. Defaults to 200 and is capped at 2000.
    #[serde(default = "default_max_results")]
    pub max_results: usize,
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

    fn permission_analysis(
        &self,
        session: &ToolSessionContext,
        input: &Self::Input,
    ) -> InvocationAnalysis {
        let display = format!("glob {} in {}", input.pattern, input.path);
        InvocationAnalysis::new(
            display.clone(),
            vec![AnalysisUnit::new(
                display,
                vec![Effect::read(session.normalize_effect_path(&input.path))],
            )],
        )
    }

    async fn execute(
        &self,
        session: &ToolSessionContext,
        call: ToolCallContext,
        input: GlobInput,
    ) -> Result<TextToolOutput, ToolExecutionError> {
        if input.max_results == 0 || input.max_results > MAX_RESULTS {
            return Err(ToolExecutionError::invalid_arguments(format!(
                "maxResults must be between 1 and {MAX_RESULTS}"
            )));
        }
        let glob = GlobPattern::new(&input.pattern).map_err(|error| {
            ToolExecutionError::invalid_arguments(format!("invalid glob: {error}"))
        })?;
        let set = GlobSet::builder().add(glob).build().map_err(|error| {
            ToolExecutionError::invalid_arguments(format!("invalid glob: {error}"))
        })?;
        let root = session
            .resolve_tool_path(&input.path, AccessKind::Read, PathIntent::MustExist, &call)
            .await?;
        let mut files = session
            .filesystem
            .walk_files(root.as_path())
            .await
            .map_err(|error| ToolExecutionError::execution(format!("glob failed: {error}")))?;
        let mut matches = Vec::with_capacity(input.max_results.min(64));
        let mut scanned = 0usize;
        while matches.len() < input.max_results {
            let path = tokio::select! {
                _ = call.cancel.cancelled() => {
                    return Err(ToolExecutionError::cancelled("glob cancelled"));
                }
                path = files.next() => path
                    .map_err(|error| ToolExecutionError::execution(format!("glob failed: {error}")))?,
            };
            let Some(path) = path else {
                break;
            };
            scanned += 1;
            if scanned.is_multiple_of(250) {
                call.report_progress(crate::ToolProgress::Message {
                    message: format!("glob scanned {scanned} files"),
                });
            }
            let relative = relative_path(root.as_path(), &path);
            if set.is_match(&relative) {
                matches.push(relative.display().to_string());
            }
        }
        matches.sort();
        let limit_reached = matches.len() >= input.max_results;
        let mut output = if matches.is_empty() {
            "no files matched\n".to_string()
        } else {
            format!("{}\n", matches.join("\n"))
        };
        if limit_reached {
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

    #[tokio::test]
    async fn streams_only_the_requested_number_of_matches() {
        let workspace = TestDirectory::new("glob-limit");
        for name in ["a.rs", "b.rs", "c.rs"] {
            std::fs::write(workspace.path().join(name), name).expect("write fixture");
        }
        let session = ToolSessionContext::local(
            workspace.path().to_path_buf(),
            PermissionProfile::from_builtin_rules(workspace.path().to_path_buf()),
        );

        let result = GlobTool
            .execute(
                &session,
                ToolCallContext::new(ToolCallId::new("glob-limit"), CancellationToken::new()),
                GlobInput {
                    pattern: "*.rs".to_string(),
                    path: ".".to_string(),
                    max_results: 2,
                },
            )
            .await
            .expect("glob result")
            .into_tool_result();

        assert_eq!(
            result
                .text_content()
                .lines()
                .filter(|line| !line.starts_with('['))
                .count(),
            2
        );
        assert!(result.text_content().contains("result limit reached"));
    }

    #[tokio::test]
    async fn observes_cancellation_while_traversing() {
        let workspace = TestDirectory::new("glob-cancel");
        std::fs::write(workspace.path().join("a.rs"), "fixture").expect("write fixture");
        let session = ToolSessionContext::local(
            workspace.path().to_path_buf(),
            PermissionProfile::from_builtin_rules(workspace.path().to_path_buf()),
        );
        let cancel = CancellationToken::new();
        cancel.cancel();

        let error = GlobTool
            .execute(
                &session,
                ToolCallContext::new(ToolCallId::new("glob-cancel"), cancel),
                GlobInput {
                    pattern: "*.rs".to_string(),
                    path: ".".to_string(),
                    max_results: 10,
                },
            )
            .await
            .expect_err("cancelled traversal");

        assert_eq!(error.code, ToolErrorCode::Cancelled);
    }
}
