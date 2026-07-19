use async_trait::async_trait;
use schemars::JsonSchema;
use serde::Deserialize;

use crate::policy::AccessKind;
use crate::{
    TextToolOutput, Tool, ToolCallContext, ToolExecutionError, ToolId, ToolRisk, ToolSessionContext,
};

use crate::context::PathIntent;

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
        let numbered = content
            .lines()
            .enumerate()
            .map(|(index, line)| format!("{:>6}\t{}", index + 1, line))
            .collect::<Vec<_>>()
            .join("\n");
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
                },
            )
            .await
            .expect("internal symlink is allowed")
            .into_tool_result();

        assert!(result.text_content().contains("inside"));
    }
}
