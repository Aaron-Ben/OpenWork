use std::io;
use std::path::{Path, PathBuf};

use thiserror::Error;

#[cfg(test)]
use super::SystemContextPart;
#[cfg(test)]
use openwork_models::model::ContentBlock;

const PROJECT_INSTRUCTION_FILE: &str = "AGENTS.md";
#[cfg(test)]
const PROJECT_INSTRUCTION_KEY: &str = "project/AGENTS.md";
const MAX_PROJECT_INSTRUCTION_BYTES: u64 = 64 * 1024;

/// Loads the project instructions scoped to one Session working directory.
pub(crate) struct ProjectInstructionLoader {
    working_directory: PathBuf,
    max_bytes: u64,
}

impl ProjectInstructionLoader {
    pub(crate) fn new(working_directory: impl Into<PathBuf>) -> Self {
        Self {
            working_directory: working_directory.into(),
            max_bytes: MAX_PROJECT_INSTRUCTION_BYTES,
        }
    }

    /// `AGENTS.md` 正文本身，不带 `SystemContextPart` 外壳。
    ///
    /// `None` 表示文件不存在或内容为空。`load()` 必须改成调用这里再包一层，见
    /// `user_project.rs::load_body` 的同款说明。
    ///
    /// 注意这里给的是**裸文件内容**：`<project_instructions>` 标记由
    /// `world_state/agents_md.rs` 加，不在这一层。
    pub(crate) async fn load_body(&self) -> Result<Option<String>, ProjectInstructionError> {
        let root = tokio::fs::canonicalize(&self.working_directory)
            .await
            .map_err(|source| ProjectInstructionError::WorkingDirectory {
                path: self.working_directory.clone(),
                source,
            })?;
        let path = root.join(PROJECT_INSTRUCTION_FILE);
        let metadata = match tokio::fs::symlink_metadata(&path).await {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(source) => {
                return Err(ProjectInstructionError::Inspect {
                    path: path.clone(),
                    source,
                });
            }
        };

        if metadata.file_type().is_symlink() {
            return Err(ProjectInstructionError::Symlink(path));
        }
        if !metadata.is_file() {
            return Err(ProjectInstructionError::NotAFile(path));
        }
        ensure_size(&path, metadata.len(), self.max_bytes)?;

        let canonical_path = tokio::fs::canonicalize(&path).await.map_err(|source| {
            ProjectInstructionError::Inspect {
                path: path.clone(),
                source,
            }
        })?;
        if !canonical_path.starts_with(&root) {
            return Err(ProjectInstructionError::OutsideWorkingDirectory(
                canonical_path,
            ));
        }

        let bytes = tokio::fs::read(&canonical_path).await.map_err(|source| {
            ProjectInstructionError::Read {
                path: canonical_path.clone(),
                source,
            }
        })?;
        ensure_size(&canonical_path, bytes.len() as u64, self.max_bytes)?;
        let content = String::from_utf8(bytes)
            .map_err(|_| ProjectInstructionError::NonUtf8(canonical_path.clone()))?;
        if content.trim().is_empty() {
            return Ok(None);
        }
        Ok(Some(content))
    }

    #[cfg(test)]
    async fn load(&self) -> Result<Option<SystemContextPart>, ProjectInstructionError> {
        Ok(self.load_body().await?.map(|content| {
            SystemContextPart::new(PROJECT_INSTRUCTION_KEY, vec![ContentBlock::text(content)])
        }))
    }

    #[cfg(test)]
    fn with_max_bytes(mut self, max_bytes: u64) -> Self {
        self.max_bytes = max_bytes;
        self
    }
}

fn ensure_size(path: &Path, actual: u64, maximum: u64) -> Result<(), ProjectInstructionError> {
    if actual > maximum {
        return Err(ProjectInstructionError::TooLarge {
            path: path.to_path_buf(),
            actual,
            maximum,
        });
    }
    Ok(())
}

#[derive(Debug, Error)]
pub(crate) enum ProjectInstructionError {
    #[error("failed to resolve Session working directory {path}: {source}")]
    WorkingDirectory { path: PathBuf, source: io::Error },
    #[error("failed to inspect project instruction file {path}: {source}")]
    Inspect { path: PathBuf, source: io::Error },
    #[error("project instruction file must not be a symlink: {0}")]
    Symlink(PathBuf),
    #[error("project instruction path is not a regular file: {0}")]
    NotAFile(PathBuf),
    #[error("project instruction resolved outside the Session working directory: {0}")]
    OutsideWorkingDirectory(PathBuf),
    #[error("project instruction file is too large: {path} ({actual} bytes, maximum {maximum})")]
    TooLarge {
        path: PathBuf,
        actual: u64,
        maximum: u64,
    },
    #[error("failed to read project instruction file {path}: {source}")]
    Read { path: PathBuf, source: io::Error },
    #[error("project instruction file is not valid UTF-8: {0}")]
    NonUtf8(PathBuf),
}

#[cfg(test)]
mod tests {
    use std::fs;

    use openwork_models::model::ContentBlock;
    use uuid::Uuid;

    use super::*;

    struct TestWorkspace {
        root: PathBuf,
    }

    impl TestWorkspace {
        fn new() -> Self {
            let root = std::env::temp_dir().join(format!(
                "openwork-project-instructions-{}",
                Uuid::new_v4().simple()
            ));
            fs::create_dir_all(&root).expect("workspace");
            Self { root }
        }

        fn write(&self, content: impl AsRef<[u8]>) {
            fs::write(self.root.join(PROJECT_INSTRUCTION_FILE), content).expect("instructions");
        }
    }

    impl Drop for TestWorkspace {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    #[tokio::test]
    async fn missing_file_produces_no_context() {
        let workspace = TestWorkspace::new();

        let part = ProjectInstructionLoader::new(&workspace.root)
            .load()
            .await
            .expect("context");

        assert!(part.is_none());
    }

    #[tokio::test]
    async fn loads_a_stable_project_instruction_block() {
        let workspace = TestWorkspace::new();
        workspace.write("root project rule\n");

        let part = ProjectInstructionLoader::new(&workspace.root)
            .load()
            .await
            .expect("context")
            .expect("project instruction part");

        assert_eq!(part.key, PROJECT_INSTRUCTION_KEY);
        assert_eq!(part.content, [ContentBlock::text("root project rule\n")]);
    }

    #[tokio::test]
    async fn whitespace_only_file_produces_no_context() {
        let workspace = TestWorkspace::new();
        workspace.write(" \n\t");

        let part = ProjectInstructionLoader::new(&workspace.root)
            .load()
            .await
            .expect("context");

        assert!(part.is_none());
    }

    #[tokio::test]
    async fn rejects_oversized_and_non_utf8_files() {
        let workspace = TestWorkspace::new();
        workspace.write("too long");
        let oversized = ProjectInstructionLoader::new(&workspace.root)
            .with_max_bytes(3)
            .load()
            .await;
        assert!(matches!(
            oversized,
            Err(ProjectInstructionError::TooLarge { .. })
        ));

        workspace.write([0xff, 0xfe]);
        let non_utf8 = ProjectInstructionLoader::new(&workspace.root).load().await;
        assert!(matches!(non_utf8, Err(ProjectInstructionError::NonUtf8(_))));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn rejects_symlinked_instruction_files() {
        use std::os::unix::fs::symlink;

        let workspace = TestWorkspace::new();
        let outside = std::env::temp_dir().join(format!(
            "openwork-project-instructions-outside-{}",
            Uuid::new_v4().simple()
        ));
        fs::write(&outside, "outside rule").expect("outside");
        symlink(&outside, workspace.root.join(PROJECT_INSTRUCTION_FILE)).expect("symlink");

        let result = ProjectInstructionLoader::new(&workspace.root).load().await;

        assert!(matches!(result, Err(ProjectInstructionError::Symlink(_))));
        let _ = fs::remove_file(outside);
    }
}
