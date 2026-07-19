use async_trait::async_trait;
use schemars::JsonSchema;
use serde::Deserialize;

use crate::policy::AccessKind;
use crate::{
    TextToolOutput, Tool, ToolCallContext, ToolExecutionError, ToolId, ToolRisk, ToolSessionContext,
};

use crate::context::PathIntent;

const MAX_BYTES: usize = 1024 * 1024;
const MAX_LINE_LIMIT: usize = 2000;

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ReadInput {
    /// Absolute path, or a path relative to the working directory.
    pub path: String,
    /// Zero-based line offset. Defaults to 0.
    #[serde(default)]
    pub offset: usize,
    /// Maximum number of lines to return. Omit to return all remaining lines.
    #[serde(default)]
    pub limit: Option<usize>,
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
        if input
            .limit
            .is_some_and(|limit| limit == 0 || limit > MAX_LINE_LIMIT)
        {
            return Err(ToolExecutionError::invalid_arguments(format!(
                "limit must be between 1 and {MAX_LINE_LIMIT}"
            )));
        }
        let resolved = session
            .resolve_path(&input.path, AccessKind::Read, PathIntent::MustExist)
            .await?;
        let content = session
            .filesystem
            .read_to_string_limited(resolved.as_path(), MAX_BYTES)
            .await
            .map_err(|error| {
                ToolExecutionError::execution(format!(
                    "failed to read {}: {error}",
                    resolved.as_path().display()
                ))
            })?;
        let total_lines = content.lines().count();
        let returned_lines = total_lines
            .saturating_sub(input.offset)
            .min(input.limit.unwrap_or(usize::MAX));
        let mut numbered = content
            .lines()
            .enumerate()
            .skip(input.offset)
            .take(input.limit.unwrap_or(usize::MAX))
            .map(|(index, line)| format!("{:>6}\t{}", index + 1, line))
            .collect::<Vec<_>>()
            .join("\n");
        if numbered.is_empty() && total_lines > 0 {
            numbered = format!("no lines at offset {} (total {total_lines})", input.offset);
        } else if input.offset.saturating_add(returned_lines) < total_lines {
            numbered.push_str(&format!(
                "\n[more lines available at offset {}]",
                input.offset.saturating_add(returned_lines)
            ));
        }
        Ok(TextToolOutput::new(numbered))
    }
}

#[cfg(all(test, unix))]
mod tests {
    use std::os::unix::fs::symlink;

    use tokio_util::sync::CancellationToken;

    use super::super::test_support::TestDirectory;
    use super::*;
    use crate::{PermissionProfile, ToolCallId, ToolErrorCode};

    #[tokio::test]
    async fn rejects_read_through_symlink_outside_workspace() {
        let sandbox = TestDirectory::new("read-symlink");
        let workspace = sandbox.path().join("workspace");
        let outside = sandbox.path().join("outside");
        std::fs::create_dir_all(&workspace).expect("create workspace");
        std::fs::create_dir_all(&outside).expect("create outside directory");
        std::fs::write(outside.join("secret.txt"), "secret").expect("write outside file");
        symlink(&outside, workspace.join("escape")).expect("create symlink");

        let session = ToolSessionContext::local(
            workspace.clone(),
            PermissionProfile::workspace_write(workspace),
        );
        let error = ReadTool
            .execute(
                &session,
                ToolCallContext::new(ToolCallId::new("read-symlink"), CancellationToken::new()),
                ReadInput {
                    path: "escape/secret.txt".to_string(),
                    offset: 0,
                    limit: None,
                },
            )
            .await
            .expect_err("symlink escape must be denied");

        assert_eq!(error.code, ToolErrorCode::PermissionDenied);
    }

    #[tokio::test]
    async fn allows_read_through_symlink_that_stays_inside_workspace() {
        use crate::ToolOutput;

        let sandbox = TestDirectory::new("read-internal-symlink");
        let workspace = sandbox.path().join("workspace");
        std::fs::create_dir_all(workspace.join("real")).expect("create workspace");
        std::fs::write(workspace.join("real/file.txt"), "inside").expect("write file");
        symlink(workspace.join("real"), workspace.join("alias")).expect("create symlink");

        let session = ToolSessionContext::local(
            workspace.clone(),
            PermissionProfile::workspace_write(workspace),
        );
        let result = ReadTool
            .execute(
                &session,
                ToolCallContext::new(
                    ToolCallId::new("read-internal-symlink"),
                    CancellationToken::new(),
                ),
                ReadInput {
                    path: "alias/file.txt".to_string(),
                    offset: 0,
                    limit: None,
                },
            )
            .await
            .expect("internal symlink is allowed")
            .into_tool_result();

        assert!(result.text_content().contains("inside"));
    }

    #[tokio::test]
    async fn reads_a_zero_based_line_page_with_original_line_numbers() {
        use crate::ToolOutput;

        let workspace = TestDirectory::new("read-page");
        std::fs::write(workspace.path().join("page.txt"), "one\ntwo\nthree\nfour\n")
            .expect("write fixture");
        let session = ToolSessionContext::local(
            workspace.path().to_path_buf(),
            PermissionProfile::workspace_write(workspace.path().to_path_buf()),
        );

        let result = ReadTool
            .execute(
                &session,
                ToolCallContext::new(ToolCallId::new("read-page"), CancellationToken::new()),
                ReadInput {
                    path: "page.txt".to_string(),
                    offset: 1,
                    limit: Some(2),
                },
            )
            .await
            .expect("read page")
            .into_tool_result();

        assert_eq!(
            result.text_content(),
            "     2\ttwo\n     3\tthree\n[more lines available at offset 3]"
        );
    }
}
