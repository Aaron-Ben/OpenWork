use std::collections::HashMap;
use std::io;
use std::path::{Path, PathBuf};

use openwork_models::model::ToolResultArtifact;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::context::PathIntent;
use crate::policy::AccessKind;
use crate::{AtomicWriteCondition, AtomicWriteError, ToolSessionContext};

pub(crate) const FILE_CHANGE_ARTIFACT_KIND: &str = "file_change";
const DIFF_CONTEXT_LINES: usize = 3;
const MAX_UNDO_FILE_BYTES: usize = 1024 * 1024;
const MAX_MYERS_TRACE_CELLS: usize = 4_000_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FileChangeKind {
    Created,
    Modified,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FileDiffLineKind {
    Context,
    Addition,
    Deletion,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileDiffLine {
    pub kind: FileDiffLineKind,
    pub old_line: Option<u32>,
    pub new_line: Option<u32>,
    pub content: String,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub no_newline: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileDiffHunk {
    pub old_start: u32,
    pub old_lines: u32,
    pub new_start: u32,
    pub new_lines: u32,
    pub lines: Vec<FileDiffLine>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileChangeArtifact {
    pub change_id: String,
    pub path: String,
    pub kind: FileChangeKind,
    pub additions: u32,
    pub deletions: u32,
    pub hunks: Vec<FileDiffHunk>,
    pub before_hash: Option<String>,
    pub after_hash: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub before_content: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub after_content: Option<String>,
    #[serde(default)]
    pub undone: bool,
}

impl FileChangeArtifact {
    pub fn from_result_artifact(artifact: &ToolResultArtifact) -> Result<Self, serde_json::Error> {
        if artifact.kind != FILE_CHANGE_ARTIFACT_KIND {
            return serde_json::from_value(serde_json::Value::Null);
        }
        serde_json::from_value(artifact.payload.clone())
    }

    pub fn to_result_artifact(&self) -> Result<ToolResultArtifact, serde_json::Error> {
        Ok(ToolResultArtifact {
            kind: FILE_CHANGE_ARTIFACT_KIND.to_string(),
            payload: serde_json::to_value(self)?,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UndoFileChangesResult {
    pub undone_change_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReapplyFileChangesResult {
    pub reapplied_change_ids: Vec<String>,
}

#[derive(Debug, Error)]
pub enum FileChangeUndoError {
    #[error("file change has already been undone: {change_id}")]
    AlreadyUndone { change_id: String },
    #[error("file change is invalid: {change_id}: {message}")]
    InvalidArtifact { change_id: String, message: String },
    #[error("file changed after the recorded edit: {path}")]
    Conflict { path: String },
    #[error("failed to restore {path}: {message}")]
    Io { path: String, message: String },
    #[error("undo failed and rollback was incomplete: {message}")]
    RollbackFailed { message: String },
}

#[derive(Debug, Error)]
pub enum FileChangeReapplyError {
    #[error("file change has not been undone: {change_id}")]
    NotUndone { change_id: String },
    #[error("file change is invalid: {change_id}: {message}")]
    InvalidArtifact { change_id: String, message: String },
    #[error("file changed after the recorded undo: {path}")]
    Conflict { path: String },
    #[error("failed to reapply {path}: {message}")]
    Io { path: String, message: String },
    #[error("reapply failed and rollback was incomplete: {message}")]
    RollbackFailed { message: String },
}

pub(crate) fn build_file_change(
    change_id: impl Into<String>,
    path: &Path,
    before: Option<&str>,
    after: &str,
) -> Option<FileChangeArtifact> {
    if before.is_some_and(|content| content == after) {
        return None;
    }
    let before_lines = before.map(split_lines).unwrap_or_default();
    let after_lines = split_lines(after);
    let operations = diff_lines(&before_lines, &after_lines);
    let additions = operations
        .iter()
        .filter(|operation| matches!(operation, DiffOperation::Addition(_)))
        .count() as u32;
    let deletions = operations
        .iter()
        .filter(|operation| matches!(operation, DiffOperation::Deletion(_)))
        .count() as u32;

    Some(FileChangeArtifact {
        change_id: change_id.into(),
        path: path.to_string_lossy().into_owned(),
        kind: if before.is_some() {
            FileChangeKind::Modified
        } else {
            FileChangeKind::Created
        },
        additions,
        deletions,
        hunks: build_hunks(&operations),
        before_hash: before.map(content_hash),
        after_hash: content_hash(after),
        before_content: before.map(str::to_string),
        after_content: Some(after.to_string()),
        undone: false,
    })
}

pub async fn undo_file_changes(
    session: &ToolSessionContext,
    changes: &[FileChangeArtifact],
) -> Result<UndoFileChangesResult, FileChangeUndoError> {
    if changes.is_empty() {
        return Ok(UndoFileChangesResult {
            undone_change_ids: Vec::new(),
        });
    }
    for change in changes {
        validate_change(change)?;
    }

    let mut checked_paths = Vec::with_capacity(changes.len());
    for change in changes {
        let checked = session
            .resolve_path(&change.path, AccessKind::Write, PathIntent::MustExist)
            .await
            .map_err(|error| FileChangeUndoError::Io {
                path: change.path.clone(),
                message: error.to_string(),
            })?;
        checked_paths.push(checked);
    }

    let mut unique_paths = checked_paths.clone();
    unique_paths.sort_by(|left, right| left.as_path().cmp(right.as_path()));
    unique_paths.dedup_by(|left, right| left.as_path() == right.as_path());
    let mut guards = Vec::with_capacity(unique_paths.len());
    for path in &unique_paths {
        guards.push(session.lock_for_write(path).await);
    }

    let mut current_by_path = HashMap::<PathBuf, Option<String>>::new();
    for path in &unique_paths {
        let content = session
            .filesystem
            .read_to_string_limited(path.as_path(), MAX_UNDO_FILE_BYTES)
            .await
            .map_err(|error| FileChangeUndoError::Io {
                path: path.as_path().display().to_string(),
                message: error.to_string(),
            })?;
        current_by_path.insert(path.as_path().to_path_buf(), Some(content));
    }

    let mut simulated = current_by_path.clone();
    for (change, checked) in changes.iter().zip(&checked_paths).rev() {
        let state = simulated.get_mut(checked.as_path()).ok_or_else(|| {
            FileChangeUndoError::InvalidArtifact {
                change_id: change.change_id.clone(),
                message: "resolved path has no loaded state".to_string(),
            }
        })?;
        let current = state
            .as_deref()
            .ok_or_else(|| FileChangeUndoError::Conflict {
                path: change.path.clone(),
            })?;
        if content_hash(current) != change.after_hash {
            return Err(FileChangeUndoError::Conflict {
                path: change.path.clone(),
            });
        }
        *state = match change.kind {
            FileChangeKind::Created => None,
            FileChangeKind::Modified => Some(change.before_content.clone().ok_or_else(|| {
                FileChangeUndoError::InvalidArtifact {
                    change_id: change.change_id.clone(),
                    message: "modified change is missing before content".to_string(),
                }
            })?),
        };
    }

    let mut applied = Vec::<AppliedUndo>::new();
    let mut undone_change_ids = Vec::with_capacity(changes.len());
    for (change, checked) in changes.iter().zip(&checked_paths).rev() {
        let current = current_by_path
            .get(checked.as_path())
            .and_then(Option::as_ref)
            .cloned()
            .ok_or_else(|| FileChangeUndoError::Conflict {
                path: change.path.clone(),
            })?;
        let restored = match change.kind {
            FileChangeKind::Created => None,
            FileChangeKind::Modified => change.before_content.clone(),
        };
        let operation =
            apply_restore(session, checked.as_path(), &current, restored.as_deref()).await;
        if let Err(error) = operation {
            rollback_applied(session, &applied).await?;
            return Err(error);
        }
        current_by_path.insert(checked.as_path().to_path_buf(), restored.clone());
        applied.push(AppliedUndo {
            path: checked.as_path().to_path_buf(),
            previous: current,
            restored,
        });
        undone_change_ids.push(change.change_id.clone());
    }

    drop(guards);
    Ok(UndoFileChangesResult { undone_change_ids })
}

pub async fn reapply_file_changes(
    session: &ToolSessionContext,
    changes: &[FileChangeArtifact],
) -> Result<ReapplyFileChangesResult, FileChangeReapplyError> {
    if changes.is_empty() {
        return Ok(ReapplyFileChangesResult {
            reapplied_change_ids: Vec::new(),
        });
    }
    for change in changes {
        validate_reapply_change(change)?;
    }

    let mut checked_paths = Vec::with_capacity(changes.len());
    for change in changes {
        let checked = session
            .resolve_path(&change.path, AccessKind::Write, PathIntent::MayCreate)
            .await
            .map_err(|error| FileChangeReapplyError::Io {
                path: change.path.clone(),
                message: error.to_string(),
            })?;
        checked_paths.push(checked);
    }

    let mut unique_paths = checked_paths.clone();
    unique_paths.sort_by(|left, right| left.as_path().cmp(right.as_path()));
    unique_paths.dedup_by(|left, right| left.as_path() == right.as_path());
    let mut guards = Vec::with_capacity(unique_paths.len());
    for path in &unique_paths {
        guards.push(session.lock_for_write(path).await);
    }

    let mut current_by_path = HashMap::<PathBuf, Option<String>>::new();
    for path in &unique_paths {
        let content = match session
            .filesystem
            .read_to_string_limited(path.as_path(), MAX_UNDO_FILE_BYTES)
            .await
        {
            Ok(content) => Some(content),
            Err(error) if error.kind() == io::ErrorKind::NotFound => None,
            Err(error) => {
                return Err(FileChangeReapplyError::Io {
                    path: path.as_path().display().to_string(),
                    message: error.to_string(),
                });
            }
        };
        current_by_path.insert(path.as_path().to_path_buf(), content);
    }

    let mut simulated = current_by_path.clone();
    for (change, checked) in changes.iter().zip(&checked_paths) {
        let state = simulated.get_mut(checked.as_path()).ok_or_else(|| {
            FileChangeReapplyError::InvalidArtifact {
                change_id: change.change_id.clone(),
                message: "resolved path has no loaded state".to_string(),
            }
        })?;
        match change.kind {
            FileChangeKind::Created if state.is_some() => {
                return Err(FileChangeReapplyError::Conflict {
                    path: change.path.clone(),
                });
            }
            FileChangeKind::Created => {}
            FileChangeKind::Modified => {
                let current = state
                    .as_deref()
                    .ok_or_else(|| FileChangeReapplyError::Conflict {
                        path: change.path.clone(),
                    })?;
                if change.before_hash.as_deref() != Some(content_hash(current).as_str()) {
                    return Err(FileChangeReapplyError::Conflict {
                        path: change.path.clone(),
                    });
                }
            }
        }
        *state = Some(after_content(change)?.to_string());
    }

    let mut applied = Vec::<AppliedReapply>::new();
    let mut reapplied_change_ids = Vec::with_capacity(changes.len());
    for (change, checked) in changes.iter().zip(&checked_paths) {
        let current = current_by_path
            .get(checked.as_path())
            .cloned()
            .ok_or_else(|| FileChangeReapplyError::InvalidArtifact {
                change_id: change.change_id.clone(),
                message: "resolved path has no loaded state".to_string(),
            })?;
        let after = after_content(change)?.to_string();
        if let Err(error) =
            apply_reapply(session, checked.as_path(), current.as_deref(), &after).await
        {
            rollback_reapplied(session, &applied).await?;
            return Err(error);
        }
        current_by_path.insert(checked.as_path().to_path_buf(), Some(after.clone()));
        applied.push(AppliedReapply {
            path: checked.as_path().to_path_buf(),
            previous: current,
            applied: after,
        });
        reapplied_change_ids.push(change.change_id.clone());
    }

    drop(guards);
    Ok(ReapplyFileChangesResult {
        reapplied_change_ids,
    })
}

fn validate_reapply_change(change: &FileChangeArtifact) -> Result<(), FileChangeReapplyError> {
    if !change.undone {
        return Err(FileChangeReapplyError::NotUndone {
            change_id: change.change_id.clone(),
        });
    }
    match change.kind {
        FileChangeKind::Created => {
            if change.before_content.is_some() || change.before_hash.is_some() {
                return Err(FileChangeReapplyError::InvalidArtifact {
                    change_id: change.change_id.clone(),
                    message: "created change unexpectedly contains a before state".to_string(),
                });
            }
        }
        FileChangeKind::Modified => {
            let before = change.before_content.as_deref().ok_or_else(|| {
                FileChangeReapplyError::InvalidArtifact {
                    change_id: change.change_id.clone(),
                    message: "modified change is missing before content".to_string(),
                }
            })?;
            if change.before_hash.as_deref() != Some(content_hash(before).as_str()) {
                return Err(FileChangeReapplyError::InvalidArtifact {
                    change_id: change.change_id.clone(),
                    message: "before content hash does not match".to_string(),
                });
            }
        }
    }
    let _ = after_content(change)?;
    Ok(())
}

fn after_content(change: &FileChangeArtifact) -> Result<&str, FileChangeReapplyError> {
    let after =
        change
            .after_content
            .as_deref()
            .ok_or_else(|| FileChangeReapplyError::InvalidArtifact {
                change_id: change.change_id.clone(),
                message: "file change is missing after content".to_string(),
            })?;
    if content_hash(after) != change.after_hash {
        return Err(FileChangeReapplyError::InvalidArtifact {
            change_id: change.change_id.clone(),
            message: "after content hash does not match".to_string(),
        });
    }
    Ok(after)
}

async fn apply_reapply(
    session: &ToolSessionContext,
    path: &Path,
    current: Option<&str>,
    after: &str,
) -> Result<(), FileChangeReapplyError> {
    let condition = current.map_or(AtomicWriteCondition::MustNotExist, |content| {
        AtomicWriteCondition::Matches(content.as_bytes().to_vec())
    });
    session
        .filesystem
        .atomic_write(path, after.as_bytes(), condition)
        .await
        .map(|_| ())
        .map_err(|error| map_reapply_atomic_error(path, error))
}

struct AppliedReapply {
    path: PathBuf,
    previous: Option<String>,
    applied: String,
}

async fn rollback_reapplied(
    session: &ToolSessionContext,
    applied: &[AppliedReapply],
) -> Result<(), FileChangeReapplyError> {
    for operation in applied.iter().rev() {
        let result = match &operation.previous {
            Some(previous) => session
                .filesystem
                .atomic_write(
                    &operation.path,
                    previous.as_bytes(),
                    AtomicWriteCondition::Matches(operation.applied.as_bytes().to_vec()),
                )
                .await
                .map(|_| ()),
            None => {
                session
                    .filesystem
                    .remove_file_if_matches(&operation.path, operation.applied.as_bytes())
                    .await
            }
        };
        result.map_err(|error| FileChangeReapplyError::RollbackFailed {
            message: format!("{}: {error}", operation.path.display()),
        })?;
    }
    Ok(())
}

fn map_reapply_atomic_error(path: &Path, error: AtomicWriteError) -> FileChangeReapplyError {
    match error {
        AtomicWriteError::Stale => FileChangeReapplyError::Conflict {
            path: path.display().to_string(),
        },
        AtomicWriteError::Io(error) => FileChangeReapplyError::Io {
            path: path.display().to_string(),
            message: error.to_string(),
        },
    }
}

fn validate_change(change: &FileChangeArtifact) -> Result<(), FileChangeUndoError> {
    if change.undone {
        return Err(FileChangeUndoError::AlreadyUndone {
            change_id: change.change_id.clone(),
        });
    }
    match change.kind {
        FileChangeKind::Created => {
            if change.before_content.is_some() || change.before_hash.is_some() {
                return Err(FileChangeUndoError::InvalidArtifact {
                    change_id: change.change_id.clone(),
                    message: "created change unexpectedly contains a before state".to_string(),
                });
            }
        }
        FileChangeKind::Modified => {
            let before = change.before_content.as_deref().ok_or_else(|| {
                FileChangeUndoError::InvalidArtifact {
                    change_id: change.change_id.clone(),
                    message: "modified change is missing before content".to_string(),
                }
            })?;
            if change.before_hash.as_deref() != Some(content_hash(before).as_str()) {
                return Err(FileChangeUndoError::InvalidArtifact {
                    change_id: change.change_id.clone(),
                    message: "before content hash does not match".to_string(),
                });
            }
        }
    }
    Ok(())
}

async fn apply_restore(
    session: &ToolSessionContext,
    path: &Path,
    current: &str,
    restored: Option<&str>,
) -> Result<(), FileChangeUndoError> {
    let result = match restored {
        Some(content) => session
            .filesystem
            .atomic_write(
                path,
                content.as_bytes(),
                AtomicWriteCondition::Matches(current.as_bytes().to_vec()),
            )
            .await
            .map(|_| ()),
        None => {
            session
                .filesystem
                .remove_file_if_matches(path, current.as_bytes())
                .await
        }
    };
    result.map_err(|error| map_atomic_error(path, error))
}

struct AppliedUndo {
    path: PathBuf,
    previous: String,
    restored: Option<String>,
}

async fn rollback_applied(
    session: &ToolSessionContext,
    applied: &[AppliedUndo],
) -> Result<(), FileChangeUndoError> {
    for operation in applied.iter().rev() {
        let condition = match &operation.restored {
            Some(content) => AtomicWriteCondition::Matches(content.as_bytes().to_vec()),
            None => AtomicWriteCondition::MustNotExist,
        };
        session
            .filesystem
            .atomic_write(&operation.path, operation.previous.as_bytes(), condition)
            .await
            .map_err(|error| FileChangeUndoError::RollbackFailed {
                message: format!("{}: {error}", operation.path.display()),
            })?;
    }
    Ok(())
}

fn map_atomic_error(path: &Path, error: AtomicWriteError) -> FileChangeUndoError {
    match error {
        AtomicWriteError::Stale => FileChangeUndoError::Conflict {
            path: path.display().to_string(),
        },
        AtomicWriteError::Io(error) => FileChangeUndoError::Io {
            path: path.display().to_string(),
            message: error.to_string(),
        },
    }
}

fn content_hash(content: &str) -> String {
    let digest = Sha256::digest(content.as_bytes());
    let mut encoded = String::with_capacity(digest.len() * 2);
    for byte in digest {
        use std::fmt::Write;
        let _ = write!(encoded, "{byte:02x}");
    }
    encoded
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SourceLine {
    content: String,
    has_newline: bool,
}

fn split_lines(content: &str) -> Vec<SourceLine> {
    content
        .split_inclusive('\n')
        .map(|line| SourceLine {
            content: line.strip_suffix('\n').unwrap_or(line).to_string(),
            has_newline: line.ends_with('\n'),
        })
        .collect()
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum DiffOperation {
    Context(SourceLine),
    Addition(SourceLine),
    Deletion(SourceLine),
}

fn diff_lines(before: &[SourceLine], after: &[SourceLine]) -> Vec<DiffOperation> {
    myers_diff(before, after).unwrap_or_else(|| fallback_diff(before, after))
}

fn myers_diff(before: &[SourceLine], after: &[SourceLine]) -> Option<Vec<DiffOperation>> {
    let n = before.len() as isize;
    let m = after.len() as isize;
    let max = (n + m) as usize;
    if max == 0 {
        return Some(Vec::new());
    }
    let offset = max as isize + 1;
    let width = max * 2 + 3;
    let mut frontier = vec![0_isize; width];
    set_frontier(&mut frontier, 1, offset, 0);
    let mut trace = Vec::new();

    for distance in 0..=max {
        if (trace.len() + 1).saturating_mul(width) > MAX_MYERS_TRACE_CELLS {
            return None;
        }
        trace.push(frontier.clone());
        let distance = distance as isize;
        let mut diagonal = -distance;
        while diagonal <= distance {
            let mut x = if diagonal == -distance
                || (diagonal != distance
                    && get_frontier(&frontier, diagonal - 1, offset)
                        < get_frontier(&frontier, diagonal + 1, offset))
            {
                get_frontier(&frontier, diagonal + 1, offset)
            } else {
                get_frontier(&frontier, diagonal - 1, offset) + 1
            };
            let mut y = x - diagonal;
            while x < n && y < m && before[x as usize] == after[y as usize] {
                x += 1;
                y += 1;
            }
            set_frontier(&mut frontier, diagonal, offset, x);
            if x >= n && y >= m {
                return Some(backtrack_diff(
                    before,
                    after,
                    &trace,
                    distance as usize,
                    offset,
                ));
            }
            diagonal += 2;
        }
    }
    None
}

fn backtrack_diff(
    before: &[SourceLine],
    after: &[SourceLine],
    trace: &[Vec<isize>],
    final_distance: usize,
    offset: isize,
) -> Vec<DiffOperation> {
    let mut x = before.len() as isize;
    let mut y = after.len() as isize;
    let mut operations = Vec::new();

    for distance in (0..=final_distance).rev() {
        let frontier = &trace[distance];
        let diagonal = x - y;
        let distance_i = distance as isize;
        let previous_diagonal = if diagonal == -distance_i
            || (diagonal != distance_i
                && get_frontier(frontier, diagonal - 1, offset)
                    < get_frontier(frontier, diagonal + 1, offset))
        {
            diagonal + 1
        } else {
            diagonal - 1
        };
        let previous_x = get_frontier(frontier, previous_diagonal, offset);
        let previous_y = previous_x - previous_diagonal;

        while x > previous_x && y > previous_y {
            operations.push(DiffOperation::Context(before[(x - 1) as usize].clone()));
            x -= 1;
            y -= 1;
        }
        if distance == 0 {
            break;
        }
        if x == previous_x {
            operations.push(DiffOperation::Addition(after[(y - 1) as usize].clone()));
            y -= 1;
        } else {
            operations.push(DiffOperation::Deletion(before[(x - 1) as usize].clone()));
            x -= 1;
        }
    }
    operations.reverse();
    operations
}

fn get_frontier(frontier: &[isize], diagonal: isize, offset: isize) -> isize {
    frontier[(diagonal + offset) as usize]
}

fn set_frontier(frontier: &mut [isize], diagonal: isize, offset: isize, value: isize) {
    frontier[(diagonal + offset) as usize] = value;
}

fn fallback_diff(before: &[SourceLine], after: &[SourceLine]) -> Vec<DiffOperation> {
    let prefix = before
        .iter()
        .zip(after)
        .take_while(|(left, right)| left == right)
        .count();
    let suffix = before[prefix..]
        .iter()
        .rev()
        .zip(after[prefix..].iter().rev())
        .take_while(|(left, right)| left == right)
        .count();
    let mut operations = Vec::with_capacity(before.len() + after.len());
    operations.extend(before[..prefix].iter().cloned().map(DiffOperation::Context));
    operations.extend(
        before[prefix..before.len().saturating_sub(suffix)]
            .iter()
            .cloned()
            .map(DiffOperation::Deletion),
    );
    operations.extend(
        after[prefix..after.len().saturating_sub(suffix)]
            .iter()
            .cloned()
            .map(DiffOperation::Addition),
    );
    operations.extend(
        before[before.len().saturating_sub(suffix)..]
            .iter()
            .cloned()
            .map(DiffOperation::Context),
    );
    operations
}

fn build_hunks(operations: &[DiffOperation]) -> Vec<FileDiffHunk> {
    let change_indexes = operations
        .iter()
        .enumerate()
        .filter_map(|(index, operation)| {
            (!matches!(operation, DiffOperation::Context(_))).then_some(index)
        })
        .collect::<Vec<_>>();
    if change_indexes.is_empty() {
        return Vec::new();
    }

    let mut ranges = Vec::<(usize, usize)>::new();
    for index in change_indexes {
        let start = index.saturating_sub(DIFF_CONTEXT_LINES);
        let end = operations.len().min(index + DIFF_CONTEXT_LINES + 1);
        if let Some((_, previous_end)) = ranges.last_mut()
            && start <= *previous_end
        {
            *previous_end = (*previous_end).max(end);
        } else {
            ranges.push((start, end));
        }
    }

    ranges
        .into_iter()
        .map(|(start, end)| build_hunk(operations, start, end))
        .collect()
}

fn build_hunk(operations: &[DiffOperation], start: usize, end: usize) -> FileDiffHunk {
    let mut old_line = 1_u32;
    let mut new_line = 1_u32;
    for operation in &operations[..start] {
        match operation {
            DiffOperation::Context(_) => {
                old_line += 1;
                new_line += 1;
            }
            DiffOperation::Addition(_) => new_line += 1,
            DiffOperation::Deletion(_) => old_line += 1,
        }
    }
    let old_start = old_line;
    let new_start = new_line;
    let mut lines = Vec::with_capacity(end - start);

    for operation in &operations[start..end] {
        match operation {
            DiffOperation::Context(line) => {
                lines.push(diff_line(
                    FileDiffLineKind::Context,
                    Some(old_line),
                    Some(new_line),
                    line,
                ));
                old_line += 1;
                new_line += 1;
            }
            DiffOperation::Addition(line) => {
                lines.push(diff_line(
                    FileDiffLineKind::Addition,
                    None,
                    Some(new_line),
                    line,
                ));
                new_line += 1;
            }
            DiffOperation::Deletion(line) => {
                lines.push(diff_line(
                    FileDiffLineKind::Deletion,
                    Some(old_line),
                    None,
                    line,
                ));
                old_line += 1;
            }
        }
    }

    let old_lines = lines.iter().filter(|line| line.old_line.is_some()).count() as u32;
    let new_lines = lines.iter().filter(|line| line.new_line.is_some()).count() as u32;
    FileDiffHunk {
        old_start: if old_lines == 0 {
            old_start.saturating_sub(1)
        } else {
            old_start
        },
        old_lines,
        new_start: if new_lines == 0 {
            new_start.saturating_sub(1)
        } else {
            new_start
        },
        new_lines,
        lines,
    }
}

fn diff_line(
    kind: FileDiffLineKind,
    old_line: Option<u32>,
    new_line: Option<u32>,
    line: &SourceLine,
) -> FileDiffLine {
    FileDiffLine {
        kind,
        old_line,
        new_line,
        content: line.content.clone(),
        no_newline: !line.has_newline,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn newline_only_change_is_not_lost() {
        let change = build_file_change("change", Path::new("file.txt"), Some("value"), "value\n")
            .expect("change");

        assert_eq!((change.additions, change.deletions), (1, 1));
    }

    #[test]
    fn empty_hunk_sides_use_unified_diff_zero_length_coordinates() {
        let created = build_file_change("created", Path::new("file.txt"), None, "one\ntwo\n")
            .expect("created change");
        assert_eq!(
            (
                created.hunks[0].old_start,
                created.hunks[0].old_lines,
                created.hunks[0].new_start,
                created.hunks[0].new_lines,
            ),
            (0, 0, 1, 2)
        );

        let deleted = build_file_change(
            "deleted-content",
            Path::new("file.txt"),
            Some("one\ntwo\n"),
            "",
        )
        .expect("deleted content change");
        assert_eq!(
            (
                deleted.hunks[0].old_start,
                deleted.hunks[0].old_lines,
                deleted.hunks[0].new_start,
                deleted.hunks[0].new_lines,
            ),
            (1, 2, 0, 0)
        );
    }

    #[test]
    fn distant_edits_create_separate_context_hunks() {
        let before = (1..=20)
            .map(|line| format!("line {line}\n"))
            .collect::<String>();
        let after = before
            .replace("line 2\n", "changed 2\n")
            .replace("line 19\n", "changed 19\n");
        let change = build_file_change("change", Path::new("file.txt"), Some(&before), &after)
            .expect("change");

        assert_eq!((change.additions, change.deletions), (2, 2));
        assert_eq!(change.hunks.len(), 2);
    }

    #[test]
    fn myers_operations_reconstruct_both_sides_for_small_sequences() {
        let mut cases = vec![Vec::<SourceLine>::new()];
        for length in 1..=4 {
            for mask in 0..(1 << length) {
                cases.push(
                    (0..length)
                        .map(|index| SourceLine {
                            content: if mask & (1 << index) == 0 {
                                "a".to_string()
                            } else {
                                "b".to_string()
                            },
                            has_newline: true,
                        })
                        .collect(),
                );
            }
        }

        for before in &cases {
            for after in &cases {
                let operations = diff_lines(before, after);
                let reconstructed_before = operations
                    .iter()
                    .filter_map(|operation| match operation {
                        DiffOperation::Context(line) | DiffOperation::Deletion(line) => {
                            Some(line.clone())
                        }
                        DiffOperation::Addition(_) => None,
                    })
                    .collect::<Vec<_>>();
                let reconstructed_after = operations
                    .iter()
                    .filter_map(|operation| match operation {
                        DiffOperation::Context(line) | DiffOperation::Addition(line) => {
                            Some(line.clone())
                        }
                        DiffOperation::Deletion(_) => None,
                    })
                    .collect::<Vec<_>>();

                assert_eq!(&reconstructed_before, before);
                assert_eq!(&reconstructed_after, after);
            }
        }
    }
}
