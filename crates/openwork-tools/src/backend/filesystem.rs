use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use async_trait::async_trait;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use super::walk::local_file_walk;
use super::{
    AsyncFileSystem, AtomicWriteCondition, AtomicWriteError, AtomicWriteOutcome, FileSystemEntry,
    FileWalk,
};

#[derive(Debug, Default)]
pub struct LocalFileSystem;

#[async_trait]
impl AsyncFileSystem for LocalFileSystem {
    async fn read_to_string(&self, path: &Path) -> io::Result<String> {
        tokio::fs::read_to_string(path).await
    }

    async fn read_to_string_limited(&self, path: &Path, max_bytes: usize) -> io::Result<String> {
        let metadata = tokio::fs::metadata(path).await?;
        if metadata.len() > max_bytes as u64 {
            return Err(file_too_large(metadata.len(), max_bytes));
        }

        let file = tokio::fs::File::open(path).await?;
        let mut bytes = Vec::with_capacity(metadata.len() as usize);
        file.take(max_bytes.saturating_add(1) as u64)
            .read_to_end(&mut bytes)
            .await?;
        if bytes.len() > max_bytes {
            return Err(file_too_large(bytes.len() as u64, max_bytes));
        }
        String::from_utf8(bytes).map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
    }

    async fn atomic_write(
        &self,
        path: &Path,
        content: &[u8],
        condition: AtomicWriteCondition,
    ) -> Result<AtomicWriteOutcome, AtomicWriteError> {
        atomic_write(path, content, condition).await
    }

    async fn create_dir_all(&self, path: &Path) -> io::Result<()> {
        tokio::fs::create_dir_all(path).await
    }

    async fn canonicalize(&self, path: &Path) -> io::Result<PathBuf> {
        tokio::fs::canonicalize(path).await
    }

    async fn is_symlink(&self, path: &Path) -> io::Result<bool> {
        tokio::fs::symlink_metadata(path)
            .await
            .map(|metadata| metadata.file_type().is_symlink())
    }

    async fn read_dir(&self, path: &Path) -> io::Result<Vec<FileSystemEntry>> {
        let mut directory = tokio::fs::read_dir(path).await?;
        let mut entries = Vec::new();
        while let Some(entry) = directory.next_entry().await? {
            entries.push(FileSystemEntry {
                name: entry.file_name().to_string_lossy().into_owned(),
                is_directory: entry.file_type().await?.is_dir(),
            });
        }
        Ok(entries)
    }

    async fn walk_files(&self, root: &Path) -> io::Result<FileWalk> {
        Ok(local_file_walk(root.to_path_buf()))
    }
}

fn file_too_large(actual_bytes: u64, max_bytes: usize) -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        format!("file too large: {actual_bytes} bytes (max {max_bytes})"),
    )
}

async fn atomic_write(
    path: &Path,
    content: &[u8],
    condition: AtomicWriteCondition,
) -> Result<AtomicWriteOutcome, AtomicWriteError> {
    let parent = path.parent().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("path has no parent directory: {}", path.display()),
        )
    })?;
    let (temporary_path, mut temporary_file) = create_temporary_file(parent, path).await?;

    let commit_result = async {
        temporary_file.write_all(content).await?;
        temporary_file.flush().await?;
        if let Ok(metadata) = tokio::fs::metadata(path).await {
            temporary_file
                .set_permissions(metadata.permissions())
                .await?;
        }
        temporary_file.sync_all().await?;

        let outcome = match condition {
            AtomicWriteCondition::Any => match compare_existing(path, content).await? {
                ExistingComparison::Missing => AtomicWriteOutcome::Created,
                ExistingComparison::Matches => return Ok(AtomicWriteOutcome::Unchanged),
                ExistingComparison::Differs => AtomicWriteOutcome::Overwritten,
            },
            AtomicWriteCondition::MustNotExist => {
                if path_exists(path).await? {
                    return Err(AtomicWriteError::Stale);
                }
                AtomicWriteOutcome::Created
            }
            AtomicWriteCondition::Matches(expected) => {
                match compare_existing(path, &expected).await? {
                    ExistingComparison::Matches => {
                        if expected == content {
                            return Ok(AtomicWriteOutcome::Unchanged);
                        }
                        AtomicWriteOutcome::Overwritten
                    }
                    ExistingComparison::Missing | ExistingComparison::Differs => {
                        return Err(AtomicWriteError::Stale);
                    }
                }
            }
        };

        drop(temporary_file);
        tokio::fs::rename(&temporary_path, path).await?;
        Ok(outcome)
    }
    .await;

    let _ = tokio::fs::remove_file(&temporary_path).await;
    commit_result
}

enum ExistingComparison {
    Missing,
    Matches,
    Differs,
}

async fn compare_existing(path: &Path, expected: &[u8]) -> io::Result<ExistingComparison> {
    let metadata = match tokio::fs::metadata(path).await {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Ok(ExistingComparison::Missing);
        }
        Err(error) => return Err(error),
    };
    if metadata.len() != expected.len() as u64 {
        return Ok(ExistingComparison::Differs);
    }
    let current = tokio::fs::read(path).await?;
    Ok(if current == expected {
        ExistingComparison::Matches
    } else {
        ExistingComparison::Differs
    })
}

async fn path_exists(path: &Path) -> io::Result<bool> {
    match tokio::fs::metadata(path).await {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error),
    }
}

async fn create_temporary_file(
    parent: &Path,
    target: &Path,
) -> io::Result<(PathBuf, tokio::fs::File)> {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let target_name = target
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("file");

    for _ in 0..32 {
        let id = COUNTER.fetch_add(1, Ordering::Relaxed);
        let path = parent.join(format!(
            ".{target_name}.openwork-{}-{id}.tmp",
            std::process::id()
        ));
        match tokio::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .await
        {
            Ok(file) => return Ok((path, file)),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    }

    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        format!(
            "failed to allocate a unique temporary file for {}",
            target.display()
        ),
    ))
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::*;

    fn temp_path(label: &str) -> PathBuf {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let id = COUNTER.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "openwork-backend-{label}-{}-{id}.txt",
            std::process::id()
        ))
    }

    #[tokio::test]
    async fn bounded_text_read_rejects_before_returning_oversized_content() {
        let path = temp_path("bounded-read");
        std::fs::write(&path, b"12345").expect("write fixture");

        let error = LocalFileSystem
            .read_to_string_limited(&path, 4)
            .await
            .expect_err("oversized file");

        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn atomic_write_rejects_stale_content_without_overwriting() {
        let path = temp_path("stale-write");
        std::fs::write(&path, b"changed externally").expect("write fixture");

        let error = LocalFileSystem
            .atomic_write(
                &path,
                b"replacement",
                AtomicWriteCondition::Matches(b"original".to_vec()),
            )
            .await
            .expect_err("stale write");

        assert!(matches!(error, AtomicWriteError::Stale));
        assert_eq!(
            std::fs::read_to_string(&path).expect("read unchanged file"),
            "changed externally"
        );
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn atomic_write_can_require_a_missing_target() {
        let path = temp_path("missing-write");
        std::fs::write(&path, b"already exists").expect("write fixture");

        let error = LocalFileSystem
            .atomic_write(&path, b"replacement", AtomicWriteCondition::MustNotExist)
            .await
            .expect_err("existing target");

        assert!(matches!(error, AtomicWriteError::Stale));
        assert_eq!(
            std::fs::read_to_string(&path).expect("read unchanged file"),
            "already exists"
        );
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn atomic_write_does_not_treat_length_mismatch_as_empty_content() {
        let path = temp_path("empty-overwrite");
        std::fs::write(&path, b"not empty").expect("write fixture");

        let outcome = LocalFileSystem
            .atomic_write(&path, b"", AtomicWriteCondition::Any)
            .await
            .expect("overwrite with empty content");

        assert_eq!(outcome, AtomicWriteOutcome::Overwritten);
        assert_eq!(std::fs::read(&path).expect("read empty file"), b"");
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn file_walk_yields_entries_incrementally() {
        let root = temp_path("walk-root");
        std::fs::create_dir_all(&root).expect("create walk root");
        std::fs::write(root.join("one.txt"), "one").expect("write fixture");
        std::fs::write(root.join("two.txt"), "two").expect("write fixture");

        let mut walk = LocalFileSystem.walk_files(&root).await.expect("start walk");
        let first = walk.next().await.expect("next file").expect("a file");

        assert!(first.starts_with(&root));
        drop(walk);
        let _ = std::fs::remove_dir_all(root);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn atomic_write_preserves_existing_unix_permissions() {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};

        let path = temp_path("permissions");
        std::fs::write(&path, b"before").expect("write fixture");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o640))
            .expect("set fixture permissions");

        LocalFileSystem
            .atomic_write(&path, b"after", AtomicWriteCondition::Any)
            .await
            .expect("atomic overwrite");

        let mode = std::fs::metadata(&path).expect("metadata").mode() & 0o777;
        assert_eq!(mode, 0o640);
        let _ = std::fs::remove_file(path);
    }
}
