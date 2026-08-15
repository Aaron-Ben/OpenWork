use std::ffi::OsStr;
use std::io;
use std::path::{Path, PathBuf};

use thiserror::Error;

const MAX_LAYOUT_ENTRIES: usize = 64;
const MAX_CONTEXT_CHARS: usize = 16 * 1024;

pub(crate) struct UserProjectContextLoader {
    working_directory: PathBuf,
}

impl UserProjectContextLoader {
    pub(crate) fn new(working_directory: impl Into<PathBuf>) -> Self {
        Self {
            working_directory: working_directory.into(),
        }
    }

    /// `<user_project_context>` 正文。
    ///
    /// 唯一的消费者是 `context/world_state/capture.rs`。这段内容曾经是 System
    /// 前缀的一部分，现在作为 world-state section 进入 Conversation。
    pub(crate) async fn load_body(&self) -> Result<String, UserProjectContextError> {
        let working_directory = tokio::fs::canonicalize(&self.working_directory)
            .await
            .map_err(|source| UserProjectContextError::WorkingDirectory {
                path: self.working_directory.clone(),
                source,
            })?;
        let repository_root = find_repository_root(&working_directory).await?;
        let layout_root = repository_root.as_deref().unwrap_or(&working_directory);
        let layout = load_top_level_layout(layout_root).await?;

        let mut text = String::from("<user_project_context format_version=\"1\">\n");
        text.push_str("Working directory: ");
        text.push_str(&escape_xml(&working_directory.to_string_lossy()));
        text.push('\n');
        text.push_str("Repository root: ");
        match &repository_root {
            Some(root) => text.push_str(&escape_xml(&root.to_string_lossy())),
            None => text.push_str("None detected"),
        }
        text.push_str("\nProject layout:\n");
        if layout.entries.is_empty() {
            text.push_str("- None\n");
        } else {
            for entry in layout.entries {
                text.push_str("- ");
                text.push_str(&escape_xml(&entry));
                text.push('\n');
            }
        }
        if layout.omitted > 0 {
            text.push_str("- ... ");
            text.push_str(&layout.omitted.to_string());
            text.push_str(" additional entries omitted\n");
        }
        text.push_str("</user_project_context>");
        if text.chars().count() > MAX_CONTEXT_CHARS {
            return Err(UserProjectContextError::TooLarge(MAX_CONTEXT_CHARS));
        }
        Ok(text)
    }
}

struct TopLevelLayout {
    entries: Vec<String>,
    omitted: usize,
}

async fn find_repository_root(
    working_directory: &Path,
) -> Result<Option<PathBuf>, UserProjectContextError> {
    for candidate in working_directory.ancestors() {
        let marker = candidate.join(".git");
        match tokio::fs::symlink_metadata(&marker).await {
            Ok(_) => return Ok(Some(candidate.to_path_buf())),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(source) => {
                return Err(UserProjectContextError::Inspect {
                    path: marker,
                    source,
                });
            }
        }
    }
    Ok(None)
}

async fn load_top_level_layout(root: &Path) -> Result<TopLevelLayout, UserProjectContextError> {
    let mut directory = tokio::fs::read_dir(root).await.map_err(|source| {
        UserProjectContextError::ReadDirectory {
            path: root.to_path_buf(),
            source,
        }
    })?;
    let mut entries: Vec<String> = Vec::new();
    while let Some(entry) =
        directory
            .next_entry()
            .await
            .map_err(|source| UserProjectContextError::ReadDirectory {
                path: root.to_path_buf(),
                source,
            })?
    {
        if entry.file_name() == OsStr::new(".git") {
            continue;
        }
        let Some(mut name) = entry.file_name().to_str().map(str::to_string) else {
            continue;
        };
        let file_type =
            entry
                .file_type()
                .await
                .map_err(|source| UserProjectContextError::Inspect {
                    path: entry.path(),
                    source,
                })?;
        if file_type.is_dir() {
            name.push('/');
        } else if file_type.is_symlink() {
            name.push('@');
        }
        entries.push(name);
    }
    entries.sort();
    let omitted = entries.len().saturating_sub(MAX_LAYOUT_ENTRIES);
    entries.truncate(MAX_LAYOUT_ENTRIES);
    Ok(TopLevelLayout { entries, omitted })
}

fn escape_xml(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            character if character.is_control() => escaped.push('\u{fffd}'),
            character => escaped.push(character),
        }
    }
    escaped
}

#[derive(Debug, Error)]
pub(crate) enum UserProjectContextError {
    #[error("failed to resolve Session working directory {path}: {source}")]
    WorkingDirectory { path: PathBuf, source: io::Error },
    #[error("failed to inspect project context path {path}: {source}")]
    Inspect { path: PathBuf, source: io::Error },
    #[error("failed to read project layout {path}: {source}")]
    ReadDirectory { path: PathBuf, source: io::Error },
    #[error("user/project context exceeded {0} characters")]
    TooLarge(usize),
}

#[cfg(test)]
mod tests {
    use std::fs;

    use uuid::Uuid;

    use super::*;

    #[tokio::test]
    async fn renders_repository_root_and_sorted_bounded_layout() {
        let root = std::env::temp_dir().join(format!(
            "openwork-user-project-context-{}",
            Uuid::new_v4().simple()
        ));
        fs::create_dir_all(root.join(".git")).expect("git marker");
        fs::create_dir_all(root.join("z-dir")).expect("directory");
        fs::create_dir_all(root.join("nested")).expect("nested");
        fs::write(root.join("a-file"), "a").expect("file");

        let text = UserProjectContextLoader::new(root.join("nested"))
            .load_body()
            .await
            .expect("context");
        let canonical_root = fs::canonicalize(&root).expect("canonical root");

        assert!(text.contains(&format!("Repository root: {}", canonical_root.display())));
        assert!(text.find("- a-file").unwrap() < text.find("- nested/").unwrap());
        assert!(text.find("- nested/").unwrap() < text.find("- z-dir/").unwrap());
        assert!(!text.contains("- .git"));

        fs::remove_dir_all(root).expect("cleanup");
    }
}
