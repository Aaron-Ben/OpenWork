use async_trait::async_trait;
use schemars::JsonSchema;
use serde::Deserialize;

use crate::policy::AccessKind;
use crate::{
    AnalysisUnit, Effect, InvocationAnalysis, TextToolOutput, Tool, ToolCallContext,
    ToolExecutionError, ToolId, ToolRisk, ToolSessionContext,
};

use crate::context::PathIntent;

const DEFAULT_LIMIT: usize = 200;
const MAX_LIMIT: usize = 2000;

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ListInput {
    /// Directory path; defaults to the working directory.
    #[serde(default = "default_path")]
    pub path: String,
    /// Zero-based entry offset after sorting. Defaults to 0.
    #[serde(default)]
    pub offset: usize,
    /// Maximum number of entries to return. Defaults to 200.
    #[serde(default = "default_limit")]
    pub limit: usize,
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

    fn permission_analysis(
        &self,
        session: &ToolSessionContext,
        input: &Self::Input,
    ) -> InvocationAnalysis {
        let display = format!("list {}", input.path);
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
        input: ListInput,
    ) -> Result<TextToolOutput, ToolExecutionError> {
        if input.limit == 0 || input.limit > MAX_LIMIT {
            return Err(ToolExecutionError::invalid_arguments(format!(
                "limit must be between 1 and {MAX_LIMIT}"
            )));
        }
        let resolved = session
            .resolve_tool_path(&input.path, AccessKind::Read, PathIntent::MustExist, &call)
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
        let total = names.len();
        let page = names
            .into_iter()
            .skip(input.offset)
            .take(input.limit)
            .collect::<Vec<_>>();
        let mut output = if total == 0 {
            format!("{} is empty", resolved.as_path().display())
        } else if page.is_empty() {
            format!("no entries at offset {} (total {total})", input.offset)
        } else {
            page.join("\n")
        };
        let next_offset = input.offset.saturating_add(page.len());
        if next_offset < total {
            output.push_str(&format!(
                "\n[showing {} entries from offset {}; more entries available at offset {next_offset}]",
                page.len(),
                input.offset
            ));
        }
        Ok(TextToolOutput::new(output))
    }
}

fn default_path() -> String {
    ".".to_string()
}

fn default_limit() -> usize {
    DEFAULT_LIMIT
}

#[cfg(test)]
mod tests {
    use tokio_util::sync::CancellationToken;

    use super::super::test_support::TestDirectory;
    use super::*;
    use crate::{PermissionProfile, ToolCallId, ToolOutput};

    #[tokio::test]
    async fn lists_a_zero_based_sorted_page() {
        let workspace = TestDirectory::new("list-page");
        for name in ["c.txt", "a.txt", "b.txt"] {
            std::fs::write(workspace.path().join(name), name).expect("write fixture");
        }
        let session = ToolSessionContext::local(
            workspace.path().to_path_buf(),
            PermissionProfile::from_builtin_rules(workspace.path().to_path_buf()),
        );

        let result = ListTool
            .execute(
                &session,
                ToolCallContext::new(ToolCallId::new("list-page"), CancellationToken::new()),
                ListInput {
                    path: ".".to_string(),
                    offset: 1,
                    limit: 1,
                },
            )
            .await
            .expect("list page")
            .into_tool_result();

        assert!(result.text_content().starts_with("b.txt\n"));
        assert!(result.text_content().contains("more entries available"));
    }
}
