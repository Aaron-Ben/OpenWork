use std::io;
use std::path::Path;

use async_trait::async_trait;
use schemars::JsonSchema;
use serde::Deserialize;

use crate::policy::AccessKind;
use crate::{
    AnalysisUnit, AtomicWriteCondition, AtomicWriteError, AtomicWriteOutcome, Effect,
    InvocationAnalysis, Tool, ToolCallContext, ToolExecutionError, ToolId, ToolResult, ToolRisk,
    ToolSessionContext,
};

use crate::context::PathIntent;
use crate::file_change::build_file_change;

const MAX_BYTES: usize = 1024 * 1024;

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
    type Output = ToolResult;

    fn id(&self) -> ToolId {
        ToolId::new_static("write")
    }

    fn description(&self) -> &'static str {
        "Write text content to a file. Creates the file (and parent directories) if missing; overwrites if it exists."
    }

    fn risk(&self) -> ToolRisk {
        ToolRisk::WorkspaceMutation
    }

    fn permission_analysis(
        &self,
        session: &ToolSessionContext,
        input: &Self::Input,
    ) -> InvocationAnalysis {
        let display = format!("write {}", input.path);
        InvocationAnalysis::new(
            display.clone(),
            vec![AnalysisUnit::new(
                display,
                vec![Effect::write(session.normalize_effect_path(&input.path))],
            )],
        )
    }

    async fn execute(
        &self,
        session: &ToolSessionContext,
        call: ToolCallContext,
        input: WriteInput,
    ) -> Result<ToolResult, ToolExecutionError> {
        if input.content.len() > MAX_BYTES {
            return Err(ToolExecutionError::invalid_arguments(format!(
                "content too large: {} bytes (max {})",
                input.content.len(),
                MAX_BYTES
            )));
        }
        let resolved = session
            .resolve_tool_path(&input.path, AccessKind::Write, PathIntent::MayCreate, &call)
            .await?;
        if let Some(parent) = resolved.as_path().parent() {
            session
                .filesystem
                .create_dir_all(parent)
                .await
                .map_err(|error| {
                    ToolExecutionError::execution(format!("failed to create parent dirs: {error}"))
                })?;
        }
        let resolved = session
            .resolve_tool_path(&input.path, AccessKind::Write, PathIntent::MayCreate, &call)
            .await?;
        let _write_guard = session.lock_for_write(&resolved).await;
        let before = match session
            .filesystem
            .read_to_string_limited(resolved.as_path(), MAX_BYTES)
            .await
        {
            Ok(content) => Some(content),
            Err(error) if error.kind() == io::ErrorKind::NotFound => None,
            Err(error) => {
                return Err(ToolExecutionError::execution(format!(
                    "failed to read existing text file {} before write: {error}",
                    resolved.as_path().display()
                )));
            }
        };
        let condition = match &before {
            Some(content) => AtomicWriteCondition::Matches(content.as_bytes().to_vec()),
            None => AtomicWriteCondition::MustNotExist,
        };
        let outcome = session
            .filesystem
            .atomic_write(resolved.as_path(), input.content.as_bytes(), condition)
            .await
            .map_err(|error| map_atomic_write_error(resolved.as_path(), error))?;
        let message = format!(
            "{} {} bytes to {}",
            match outcome {
                AtomicWriteOutcome::Created => "created",
                AtomicWriteOutcome::Overwritten => "overwrote",
                AtomicWriteOutcome::Unchanged => "left unchanged",
            },
            input.content.len(),
            resolved.as_path().display()
        );
        let Some(change) = build_file_change(
            call.call_id.as_str(),
            Path::new(&input.path),
            before.as_deref(),
            &input.content,
        ) else {
            return Ok(ToolResult::succeeded(message));
        };
        let artifact = change.to_result_artifact().map_err(|error| {
            ToolExecutionError::execution(format!("failed to encode file change: {error}"))
        })?;
        Ok(ToolResult::succeeded_with_artifact(message, artifact))
    }
}

fn map_atomic_write_error(path: &std::path::Path, error: AtomicWriteError) -> ToolExecutionError {
    match error {
        AtomicWriteError::Stale => ToolExecutionError::execution(format!(
            "file changed before write could be committed: {}",
            path.display()
        )),
        AtomicWriteError::Io(error) => {
            ToolExecutionError::execution(format!("failed to write {}: {error}", path.display()))
        }
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
    async fn rejects_new_file_through_symlink_outside_workspace() {
        let sandbox = TestDirectory::new("write-symlink");
        let workspace = sandbox.path().join("workspace");
        let outside = sandbox.path().join("outside");
        std::fs::create_dir_all(&workspace).expect("create workspace");
        std::fs::create_dir_all(&outside).expect("create outside directory");
        symlink(&outside, workspace.join("escape")).expect("create symlink");

        let session = ToolSessionContext::local(
            workspace.clone(),
            PermissionProfile::from_builtin_rules(workspace),
        );
        let error = WriteTool
            .execute(
                &session,
                ToolCallContext::new(ToolCallId::new("write-symlink"), CancellationToken::new()),
                WriteInput {
                    path: "escape/created.txt".to_string(),
                    content: "outside".to_string(),
                },
            )
            .await
            .expect_err("symlink escape must be denied");

        assert_eq!(error.code, ToolErrorCode::PermissionDenied);
        assert!(!outside.join("created.txt").exists());
    }

    #[tokio::test]
    async fn rejects_protected_metadata_through_symlink_alias() {
        let sandbox = TestDirectory::new("write-protected-symlink");
        let workspace = sandbox.path().join("workspace");
        std::fs::create_dir_all(workspace.join(".git")).expect("create protected directory");
        symlink(workspace.join(".git"), workspace.join("metadata")).expect("create symlink");

        let session = ToolSessionContext::local(
            workspace.clone(),
            PermissionProfile::from_builtin_rules(workspace.clone()),
        );
        let error = WriteTool
            .execute(
                &session,
                ToolCallContext::new(
                    ToolCallId::new("write-protected-symlink"),
                    CancellationToken::new(),
                ),
                WriteInput {
                    path: "metadata/config".to_string(),
                    content: "unsafe".to_string(),
                },
            )
            .await
            .expect_err("protected canonical target must be denied");

        assert_eq!(error.code, ToolErrorCode::PermissionDenied);
        assert!(!workspace.join(".git/config").exists());
    }

    #[tokio::test]
    async fn rejects_new_file_through_dangling_symlink() {
        let sandbox = TestDirectory::new("write-dangling-symlink");
        let workspace = sandbox.path().join("workspace");
        let outside_target = sandbox.path().join("outside/missing-directory");
        std::fs::create_dir_all(&workspace).expect("create workspace");
        symlink(&outside_target, workspace.join("escape")).expect("create dangling symlink");

        let session = ToolSessionContext::local(
            workspace.clone(),
            PermissionProfile::from_builtin_rules(workspace),
        );
        let error = WriteTool
            .execute(
                &session,
                ToolCallContext::new(
                    ToolCallId::new("write-dangling-symlink"),
                    CancellationToken::new(),
                ),
                WriteInput {
                    path: "escape/created.txt".to_string(),
                    content: "outside".to_string(),
                },
            )
            .await
            .expect_err("dangling symlink escape must be denied");

        assert_eq!(error.code, ToolErrorCode::PermissionDenied);
        assert!(!outside_target.join("created.txt").exists());
    }
}
