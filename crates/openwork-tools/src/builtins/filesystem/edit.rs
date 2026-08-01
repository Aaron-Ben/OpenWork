use std::path::Path;

use async_trait::async_trait;
use schemars::JsonSchema;
use serde::Deserialize;

use crate::policy::AccessKind;
use crate::{
    AnalysisUnit, AsyncFileSystem, AtomicWriteCondition, AtomicWriteError, Effect,
    InvocationAnalysis, Tool, ToolCallContext, ToolExecutionError, ToolId, ToolResult, ToolRisk,
    ToolSessionContext,
};

use crate::context::PathIntent;
use crate::file_change::{FileChangeArtifact, build_file_change};

const MAX_BYTES: usize = 1024 * 1024;

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct EditInput {
    /// Absolute or working-dir-relative path.
    pub file_path: String,
    /// Exact text to find. Empty string means create a new file.
    pub old_string: String,
    /// Replacement text, or full content for a new file.
    pub new_string: String,
    /// Replace every occurrence. Defaults to false.
    #[serde(default)]
    pub replace_all: bool,
}

#[derive(Debug, Default)]
pub struct EditTool;

#[async_trait]
impl Tool for EditTool {
    type Input = EditInput;
    type Output = ToolResult;

    fn id(&self) -> ToolId {
        ToolId::new_static("edit")
    }

    fn description(&self) -> &'static str {
        "Edit a file by replacing a unique occurrence of `oldString` with `newString`. Use `oldString: \"\"` to create a new file (refuses if it already exists). Set `replaceAll: true` to replace every occurrence. Without `replaceAll`, `oldString` must match exactly and be unique in the file."
    }

    fn risk(&self) -> ToolRisk {
        ToolRisk::WorkspaceMutation
    }

    fn permission_analysis(
        &self,
        session: &ToolSessionContext,
        input: &Self::Input,
    ) -> InvocationAnalysis {
        let display = format!("edit {}", input.file_path);
        InvocationAnalysis::new(
            display.clone(),
            vec![AnalysisUnit::new(
                display,
                vec![Effect::write(
                    session.normalize_effect_path(&input.file_path),
                )],
            )],
        )
    }

    async fn execute(
        &self,
        session: &ToolSessionContext,
        call: ToolCallContext,
        input: EditInput,
    ) -> Result<ToolResult, ToolExecutionError> {
        let intent = if input.old_string.is_empty() {
            PathIntent::MayCreate
        } else {
            PathIntent::MustExist
        };
        let resolved = session
            .resolve_tool_path(&input.file_path, AccessKind::Write, intent, &call)
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
            .resolve_tool_path(&input.file_path, AccessKind::Write, intent, &call)
            .await?;
        let _write_guard = session.lock_for_write(&resolved).await;
        let (message, change) = apply_edit(
            session.filesystem.as_ref(),
            resolved.as_path(),
            Path::new(&input.file_path),
            call.call_id.as_str(),
            &input.old_string,
            &input.new_string,
            input.replace_all,
        )
        .await?;
        let artifact = change.to_result_artifact().map_err(|error| {
            ToolExecutionError::execution(format!("failed to encode file change: {error}"))
        })?;
        Ok(ToolResult::succeeded_with_artifact(message, artifact))
    }
}

async fn apply_edit(
    filesystem: &dyn AsyncFileSystem,
    path: &Path,
    artifact_path: &Path,
    change_id: &str,
    old: &str,
    new: &str,
    replace_all: bool,
) -> Result<(String, FileChangeArtifact), ToolExecutionError> {
    if old == new {
        return Err(ToolExecutionError::execution(
            "oldString and newString are identical (no-op)",
        ));
    }

    if new.len() > MAX_BYTES {
        return Err(ToolExecutionError::invalid_arguments(format!(
            "newString too large: {} bytes (max {})",
            new.len(),
            MAX_BYTES
        )));
    }

    if old.is_empty() {
        filesystem
            .atomic_write(path, new.as_bytes(), AtomicWriteCondition::MustNotExist)
            .await
            .map_err(|error| map_atomic_write_error(path, error, true))?;
        let change = build_file_change(change_id, artifact_path, None, new).ok_or_else(|| {
            ToolExecutionError::execution("created file did not produce a file change")
        })?;
        return Ok((
            format!("created {} ({} bytes)", path.display(), new.len()),
            change,
        ));
    }

    let content = filesystem
        .read_to_string_limited(path, MAX_BYTES)
        .await
        .map_err(|error| {
            ToolExecutionError::execution(format!("failed to read {}: {error}", path.display()))
        })?;
    let count = content.matches(old).count();
    if count == 0 {
        return Err(ToolExecutionError::execution(format!(
            "oldString not found in {}",
            path.display()
        )));
    }

    let updated = if replace_all {
        content.replace(old, new)
    } else if count == 1 {
        content.replacen(old, new, 1)
    } else {
        return Err(ToolExecutionError::execution(format!(
            "oldString is not unique: found {} occurrences in {}; include more surrounding context or set replaceAll: true",
            count,
            path.display()
        )));
    };

    let change = build_file_change(change_id, artifact_path, Some(&content), &updated)
        .ok_or_else(|| ToolExecutionError::execution("edit did not produce a file change"))?;
    filesystem
        .atomic_write(
            path,
            updated.as_bytes(),
            AtomicWriteCondition::Matches(content.into_bytes()),
        )
        .await
        .map_err(|error| map_atomic_write_error(path, error, false))?;
    if replace_all {
        Ok((
            format!("replaced {} occurrence(s) in {}", count, path.display()),
            change,
        ))
    } else {
        Ok((format!("edited {}", path.display()), change))
    }
}

fn map_atomic_write_error(
    path: &Path,
    error: AtomicWriteError,
    creating: bool,
) -> ToolExecutionError {
    match error {
        AtomicWriteError::Stale if creating => ToolExecutionError::execution(format!(
            "file already exists or changed before creation: {}; to modify it, provide a non-empty oldString",
            path.display()
        )),
        AtomicWriteError::Stale => ToolExecutionError::execution(format!(
            "file changed while edit was being prepared: {}; retry with the latest content",
            path.display()
        )),
        AtomicWriteError::Io(error) => {
            ToolExecutionError::execution(format!("failed to write {}: {error}", path.display()))
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    use crate::LocalFileSystem;

    use super::*;

    fn temp_file() -> PathBuf {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let id = COUNTER.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!("openwork-edit-test-{id}.txt"))
    }

    #[tokio::test]
    async fn creates_and_edits_files() {
        let path = temp_file();
        let _ = std::fs::remove_file(&path);
        let filesystem = LocalFileSystem;

        let created = apply_edit(
            &filesystem,
            &path,
            &path,
            "create",
            "",
            "foo bar foo",
            false,
        )
        .await
        .expect("create file");
        assert!(created.0.contains("created"));
        let edited = apply_edit(&filesystem, &path, &path, "edit", "bar", "baz", false)
            .await
            .expect("edit file");
        assert!(edited.0.contains("edited"));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "foo baz foo");
        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn rejects_ambiguous_or_noop_edits() {
        let path = temp_file();
        std::fs::write(&path, "foo foo").unwrap();
        let filesystem = LocalFileSystem;

        assert!(
            apply_edit(&filesystem, &path, &path, "ambiguous", "foo", "x", false,)
                .await
                .expect_err("ambiguous edit")
                .message
                .contains("not unique")
        );
        assert!(
            apply_edit(&filesystem, &path, &path, "noop", "foo", "foo", false)
                .await
                .expect_err("noop edit")
                .message
                .contains("identical")
        );
        let _ = std::fs::remove_file(&path);
    }
}
