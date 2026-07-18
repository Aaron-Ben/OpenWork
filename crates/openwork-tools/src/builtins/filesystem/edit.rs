use std::path::Path;

use async_trait::async_trait;
use schemars::JsonSchema;
use serde::Deserialize;

use crate::policy::AccessKind;
use crate::{
    AsyncFileSystem, TextToolOutput, Tool, ToolCallContext, ToolExecutionError, ToolId, ToolRisk,
    ToolSessionContext,
};

use super::resolve;

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
    type Output = TextToolOutput;

    fn id(&self) -> ToolId {
        ToolId::new_static("edit")
    }

    fn description(&self) -> &'static str {
        "Edit a file by replacing a unique occurrence of `oldString` with `newString`. Use `oldString: \"\"` to create a new file (refuses if it already exists). Set `replaceAll: true` to replace every occurrence. Without `replaceAll`, `oldString` must match exactly and be unique in the file."
    }

    fn risk(&self) -> ToolRisk {
        ToolRisk::WorkspaceMutation
    }

    async fn execute(
        &self,
        session: &ToolSessionContext,
        _call: ToolCallContext,
        input: EditInput,
    ) -> Result<TextToolOutput, ToolExecutionError> {
        let resolved = resolve(&session.working_directory, &input.file_path);
        session
            .check_path(&resolved, AccessKind::Write)
            .map_err(ToolExecutionError::denied)?;
        let message = apply_edit(
            session.filesystem.as_ref(),
            &resolved,
            &input.old_string,
            &input.new_string,
            input.replace_all,
        )
        .await?;
        Ok(TextToolOutput::new(message))
    }
}

async fn apply_edit(
    filesystem: &dyn AsyncFileSystem,
    path: &Path,
    old: &str,
    new: &str,
    replace_all: bool,
) -> Result<String, ToolExecutionError> {
    if old == new {
        return Err(ToolExecutionError::execution(
            "oldString and newString are identical (no-op)",
        ));
    }

    if old.is_empty() {
        if filesystem
            .exists(path)
            .await
            .map_err(|error| ToolExecutionError::execution(error.to_string()))?
        {
            return Err(ToolExecutionError::execution(format!(
                "file already exists: {}; to modify it, provide a non-empty oldString",
                path.display()
            )));
        }
        if let Some(parent) = path.parent() {
            filesystem.create_dir_all(parent).await.map_err(|error| {
                ToolExecutionError::execution(format!("failed to create parent dirs: {error}"))
            })?;
        }
        filesystem
            .write(path, new.as_bytes())
            .await
            .map_err(|error| {
                ToolExecutionError::execution(format!(
                    "failed to write {}: {error}",
                    path.display()
                ))
            })?;
        return Ok(format!("created {} ({} bytes)", path.display(), new.len()));
    }

    let content = filesystem.read_to_string(path).await.map_err(|error| {
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

    filesystem
        .write(path, updated.as_bytes())
        .await
        .map_err(|error| {
            ToolExecutionError::execution(format!("failed to write {}: {error}", path.display()))
        })?;
    if replace_all {
        Ok(format!(
            "replaced {} occurrence(s) in {}",
            count,
            path.display()
        ))
    } else {
        Ok(format!("edited {}", path.display()))
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

        let created = apply_edit(&filesystem, &path, "", "foo bar foo", false)
            .await
            .expect("create file");
        assert!(created.contains("created"));
        let edited = apply_edit(&filesystem, &path, "bar", "baz", false)
            .await
            .expect("edit file");
        assert!(edited.contains("edited"));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "foo baz foo");
        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn rejects_ambiguous_or_noop_edits() {
        let path = temp_file();
        std::fs::write(&path, "foo foo").unwrap();
        let filesystem = LocalFileSystem;

        assert!(
            apply_edit(&filesystem, &path, "foo", "x", false)
                .await
                .expect_err("ambiguous edit")
                .message
                .contains("not unique")
        );
        assert!(
            apply_edit(&filesystem, &path, "foo", "foo", false)
                .await
                .expect_err("noop edit")
                .message
                .contains("identical")
        );
        let _ = std::fs::remove_file(&path);
    }
}
