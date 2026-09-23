use std::path::Path;

use async_trait::async_trait;
use schemars::JsonSchema;
use serde::Deserialize;

use crate::policy::AccessKind;
use crate::{
    AnalysisUnit, AsyncFileSystem, AtomicWriteCondition, AtomicWriteError, Effect,
    FileObservations, InvocationAnalysis, Tool, ToolCallContext, ToolExecutionError, ToolId,
    ToolResult, ToolRisk, ToolSessionContext,
};

use super::workspace_display;
use crate::context::PathIntent;
use crate::file_change::{FileChangeArtifact, build_file_change};
use crate::observation::content_hash;

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
        let display = workspace_display(session, resolved.as_path()).await;
        let target = EditTarget {
            path: resolved.as_path(),
            display: &display,
            artifact_path: Path::new(&input.file_path),
            change_id: call.call_id.as_str(),
            observations: &session.observations,
        };
        let (message, change) = apply_edit(
            session.filesystem.as_ref(),
            &target,
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

/// The file an edit applies to, and how it is named in the result and the
/// file-change artifact.
struct EditTarget<'a> {
    path: &'a Path,
    display: &'a str,
    artifact_path: &'a Path,
    change_id: &'a str,
    observations: &'a FileObservations,
}

async fn apply_edit(
    filesystem: &dyn AsyncFileSystem,
    target: &EditTarget<'_>,
    old: &str,
    new: &str,
    replace_all: bool,
) -> Result<(String, FileChangeArtifact), ToolExecutionError> {
    let EditTarget {
        path,
        display,
        artifact_path,
        change_id,
        observations,
    } = *target;
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
        observations.record(path, content_hash(new.as_bytes()));
        let change = build_file_change(change_id, artifact_path, None, new).ok_or_else(|| {
            ToolExecutionError::execution("created file did not produce a file change")
        })?;
        return Ok((format!("Created {display} (+{})", change.additions), change));
    }

    let content = filesystem
        .read_to_string_limited(path, MAX_BYTES)
        .await
        .map_err(|error| {
            ToolExecutionError::execution(format!("failed to read {}: {error}", path.display()))
        })?;
    observations.check_current(path, display, content.as_bytes())?;
    let count = content.matches(old).count();
    if count == 0 {
        return Err(ToolExecutionError::execution(format!(
            "oldString not found in {}",
            path.display()
        )));
    }

    let first_match = content.find(old).unwrap_or(0);
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
    let (start, end) = replaced_lines(&content[..first_match], new);
    filesystem
        .atomic_write(
            path,
            updated.as_bytes(),
            AtomicWriteCondition::Matches(content.into_bytes()),
        )
        .await
        .map_err(|error| map_atomic_write_error(path, error, false))?;
    observations.record(path, content_hash(updated.as_bytes()));
    let counts = format!("(+{} -{})", change.additions, change.deletions);
    if replace_all && count > 1 {
        return Ok((
            format!("Replaced {count} occurrences in {display} {counts}"),
            change,
        ));
    }
    let range = if start == end {
        start.to_string()
    } else {
        format!("{start}-{end}")
    };
    Ok((format!("Edited {display}:{range} {counts}"), change))
}

/// The lines the replacement occupies in the new file, so the model can
/// re-read just that range (tools.md §9 edit).
fn replaced_lines(before_match: &str, new: &str) -> (usize, usize) {
    let start = before_match.matches('\n').count() + 1;
    let spanned = new.strip_suffix('\n').unwrap_or(new).matches('\n').count();
    (start, start + spanned)
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
        std::env::temp_dir().join(format!(
            "openwork-edit-test-{}-{id}.txt",
            std::process::id()
        ))
    }

    fn target<'a>(
        path: &'a Path,
        change_id: &'a str,
        observations: &'a FileObservations,
    ) -> EditTarget<'a> {
        EditTarget {
            path,
            display: "file.rs",
            artifact_path: path,
            change_id,
            observations,
        }
    }

    /// A table in which the model has read `path` as it is now.
    fn observed(path: &Path) -> FileObservations {
        let observations = FileObservations::new();
        observations.record(path, content_hash(&std::fs::read(path).unwrap()));
        observations
    }

    /// tools.md §12 #28: one-line summaries with the new line range.
    #[tokio::test]
    async fn acc_28_edits_report_one_line_with_the_new_line_range() {
        let path = temp_file();
        let _ = std::fs::remove_file(&path);
        let filesystem = LocalFileSystem;
        let observations = FileObservations::new();

        let created = apply_edit(
            &filesystem,
            &target(&path, "create", &observations),
            "",
            "one\ntwo\nthree\nfour\n",
            false,
        )
        .await
        .expect("create file");
        assert_eq!(created.0, "Created file.rs (+4)");

        let edited = apply_edit(
            &filesystem,
            &target(&path, "edit", &observations),
            "two\nthree\n",
            "2\n2.5\n3\n",
            false,
        )
        .await
        .expect("edit file");
        assert_eq!(edited.0, "Edited file.rs:2-4 (+3 -2)");

        let single = apply_edit(
            &filesystem,
            &target(&path, "single", &observations),
            "four",
            "4",
            false,
        )
        .await
        .expect("single-line edit");
        assert_eq!(single.0, "Edited file.rs:5 (+1 -1)");
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "one\n2\n2.5\n3\n4\n"
        );
        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn replace_all_reports_the_number_of_occurrences() {
        let path = temp_file();
        std::fs::write(&path, "a\nx\na\n").unwrap();
        let observations = observed(&path);
        let replaced = apply_edit(
            &LocalFileSystem,
            &target(&path, "all", &observations),
            "a",
            "b",
            true,
        )
        .await
        .expect("replace all");
        assert_eq!(replaced.0, "Replaced 2 occurrences in file.rs (+2 -2)");
        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn rejects_ambiguous_or_noop_edits() {
        let path = temp_file();
        std::fs::write(&path, "foo foo").unwrap();
        let filesystem = LocalFileSystem;
        let observations = observed(&path);

        assert!(
            apply_edit(
                &filesystem,
                &target(&path, "ambiguous", &observations),
                "foo",
                "x",
                false
            )
            .await
            .expect_err("ambiguous edit")
            .message
            .contains("not unique")
        );
        assert!(
            apply_edit(
                &filesystem,
                &target(&path, "noop", &observations),
                "foo",
                "foo",
                false
            )
            .await
            .expect_err("noop edit")
            .message
            .contains("identical")
        );
        let _ = std::fs::remove_file(&path);
    }
}
