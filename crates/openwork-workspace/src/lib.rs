use std::collections::{HashMap, HashSet};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Serialize;
use thiserror::Error;

#[derive(Debug, Clone)]
pub struct CapturedWorktree {
    pub working_dir: PathBuf,
    pub status: Vec<GitStatusEntry>,
    pub contents: HashMap<String, Option<Vec<u8>>>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitStatusEntry {
    pub path: String,
    pub status: String,
}

#[derive(Debug, Clone)]
pub struct WorktreeFileChange {
    pub path: String,
    pub before_content: Option<Vec<u8>>,
    pub after_content: Option<Vec<u8>>,
}

#[derive(Debug, Error)]
pub enum WorkspaceError {
    #[error("workspace io error: {0}")]
    Io(#[from] io::Error),
    #[error("unsafe snapshot path: {0}")]
    UnsafePath(String),
    #[error("snapshot already reverted")]
    AlreadyReverted,
    #[error("cannot revert because file changed after snapshot: {0}")]
    FileChanged(String),
}

pub fn capture_worktree(working_dir: &Path) -> Result<CapturedWorktree, WorkspaceError> {
    let root = git_root(working_dir)?;
    let status = git_status(&root)?;
    let mut contents = HashMap::new();
    for entry in &status {
        let path = safe_worktree_path(&root, &entry.path)?;
        contents.insert(entry.path.clone(), read_optional(&path)?);
    }
    Ok(CapturedWorktree {
        working_dir: root,
        status,
        contents,
    })
}

pub fn worktree_file_changes(
    before: &CapturedWorktree,
    after: &CapturedWorktree,
) -> Result<Vec<WorktreeFileChange>, WorkspaceError> {
    let mut changes = Vec::new();
    for path in snapshot_paths(before, after) {
        let before_content = match before.contents.get(&path) {
            Some(content) => content.clone(),
            None => {
                let _ = safe_worktree_path(&before.working_dir, &path)?;
                read_git_head_file(&before.working_dir, &path)?
            }
        };
        let after_content = match after.contents.get(&path) {
            Some(content) => content.clone(),
            None => {
                let path = safe_worktree_path(&after.working_dir, &path)?;
                read_optional(&path)?
            }
        };
        if before_content == after_content {
            continue;
        }
        changes.push(WorktreeFileChange {
            path,
            before_content,
            after_content,
        });
    }
    changes.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(changes)
}

pub fn revert_file_changes(
    working_dir: &Path,
    files: &[WorktreeFileChange],
) -> Result<(), WorkspaceError> {
    for file in files {
        let path = safe_worktree_path(working_dir, &file.path)?;
        let current = read_optional(&path)?;
        if current != file.after_content {
            return Err(WorkspaceError::FileChanged(file.path.clone()));
        }
    }

    for file in files {
        let path = safe_worktree_path(working_dir, &file.path)?;
        match &file.before_content {
            Some(bytes) => {
                if let Some(parent) = path.parent() {
                    fs::create_dir_all(parent)?;
                }
                fs::write(&path, bytes)?;
            }
            None => {
                if path.exists() {
                    fs::remove_file(&path)?;
                }
            }
        }
    }

    Ok(())
}

pub fn bytes_to_text(bytes: Option<&[u8]>) -> Option<String> {
    let bytes = bytes?;
    if bytes.contains(&0) {
        return None;
    }
    String::from_utf8(bytes.to_vec()).ok()
}

fn snapshot_paths(before: &CapturedWorktree, after: &CapturedWorktree) -> Vec<String> {
    let mut paths = HashSet::new();
    for entry in before.status.iter().chain(after.status.iter()) {
        paths.insert(entry.path.clone());
    }
    let mut out: Vec<String> = paths.into_iter().collect();
    out.sort();
    out
}

fn git_root(working_dir: &Path) -> io::Result<PathBuf> {
    let output = Command::new("git")
        .args([
            "-C",
            &working_dir.to_string_lossy(),
            "rev-parse",
            "--show-toplevel",
        ])
        .output()?;
    if !output.status.success() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "working directory is not inside a git repository",
        ));
    }
    let text = String::from_utf8_lossy(&output.stdout);
    Ok(PathBuf::from(text.trim()))
}

fn git_status(root: &Path) -> io::Result<Vec<GitStatusEntry>> {
    let output = Command::new("git")
        .args([
            "-C",
            &root.to_string_lossy(),
            "status",
            "--porcelain=v1",
            "-z",
        ])
        .output()?;
    if !output.status.success() {
        return Err(io::Error::other("git status failed"));
    }
    let mut entries = Vec::new();
    let mut parts = output
        .stdout
        .split(|byte| *byte == 0)
        .filter(|part| !part.is_empty());
    while let Some(raw) = parts.next() {
        if raw.len() < 4 {
            continue;
        }
        let status = String::from_utf8_lossy(&raw[..2]).to_string();
        let path = String::from_utf8_lossy(&raw[3..]).to_string();
        if status.starts_with('R') || status.starts_with('C') {
            let _old_path = parts.next();
        }
        entries.push(GitStatusEntry { path, status });
    }
    Ok(entries)
}

fn read_git_head_file(root: &Path, path: &str) -> io::Result<Option<Vec<u8>>> {
    let output = Command::new("git")
        .args([
            "-C",
            &root.to_string_lossy(),
            "show",
            &format!("HEAD:{path}"),
        ])
        .output()?;
    if output.status.success() {
        Ok(Some(output.stdout))
    } else {
        Ok(None)
    }
}

fn read_optional(path: &Path) -> io::Result<Option<Vec<u8>>> {
    match fs::read(path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

fn safe_worktree_path(root: &Path, relative: &str) -> Result<PathBuf, WorkspaceError> {
    let relative_path = Path::new(relative);
    if relative_path.is_absolute()
        || relative_path
            .components()
            .any(|component| matches!(component, std::path::Component::ParentDir))
    {
        return Err(WorkspaceError::UnsafePath(relative.to_string()));
    }
    Ok(root.join(relative_path))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bytes_to_text_rejects_binary_content() {
        assert_eq!(bytes_to_text(Some(b"hello")), Some("hello".to_string()));
        assert_eq!(bytes_to_text(Some(b"hello\0world")), None);
        assert_eq!(bytes_to_text(None), None);
    }

    #[test]
    fn worktree_file_changes_detects_modified_content() {
        let before = CapturedWorktree {
            working_dir: PathBuf::from("/tmp/project"),
            status: vec![GitStatusEntry {
                path: "src/lib.rs".to_string(),
                status: " M".to_string(),
            }],
            contents: HashMap::from([("src/lib.rs".to_string(), Some(b"before".to_vec()))]),
        };
        let after = CapturedWorktree {
            working_dir: PathBuf::from("/tmp/project"),
            status: vec![GitStatusEntry {
                path: "src/lib.rs".to_string(),
                status: " M".to_string(),
            }],
            contents: HashMap::from([("src/lib.rs".to_string(), Some(b"after".to_vec()))]),
        };

        let changes = worktree_file_changes(&before, &after).expect("changes");

        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].path, "src/lib.rs");
        assert_eq!(changes[0].before_content, Some(b"before".to_vec()));
        assert_eq!(changes[0].after_content, Some(b"after".to_vec()));
    }

    #[test]
    fn worktree_file_changes_rejects_unsafe_paths() {
        let before = CapturedWorktree {
            working_dir: PathBuf::from("/tmp/project"),
            status: vec![GitStatusEntry {
                path: "../outside".to_string(),
                status: " M".to_string(),
            }],
            contents: HashMap::new(),
        };
        let after = CapturedWorktree {
            working_dir: PathBuf::from("/tmp/project"),
            status: Vec::new(),
            contents: HashMap::new(),
        };

        let error = worktree_file_changes(&before, &after).expect_err("unsafe path");

        assert!(matches!(error, WorkspaceError::UnsafePath(path) if path == "../outside"));
    }
}
