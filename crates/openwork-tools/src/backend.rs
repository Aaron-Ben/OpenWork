use std::collections::HashMap;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use ignore::WalkBuilder;
use thiserror::Error;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use crate::policy::NetworkMode;
use crate::{ToolCallContext, ToolExecutionError};

const MAX_CAPTURE_BYTES: usize = 16 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileSystemEntry {
    pub name: String,
    pub is_directory: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AtomicWriteCondition {
    Any,
    MustNotExist,
    Matches(Vec<u8>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AtomicWriteOutcome {
    Created,
    Overwritten,
    Unchanged,
}

#[derive(Debug, Error)]
pub enum AtomicWriteError {
    #[error("target changed before atomic write could be committed")]
    Stale,
    #[error(transparent)]
    Io(#[from] io::Error),
}

#[async_trait]
pub trait AsyncFileSystem: Send + Sync {
    async fn read_to_string(&self, path: &Path) -> io::Result<String>;
    async fn read_to_string_limited(&self, path: &Path, max_bytes: usize) -> io::Result<String>;
    async fn atomic_write(
        &self,
        path: &Path,
        content: &[u8],
        condition: AtomicWriteCondition,
    ) -> Result<AtomicWriteOutcome, AtomicWriteError>;
    async fn create_dir_all(&self, path: &Path) -> io::Result<()>;
    async fn canonicalize(&self, path: &Path) -> io::Result<PathBuf>;
    async fn is_symlink(&self, path: &Path) -> io::Result<bool>;
    async fn read_dir(&self, path: &Path) -> io::Result<Vec<FileSystemEntry>>;
    async fn walk_files(&self, root: &Path) -> io::Result<Vec<PathBuf>>;
}

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

    async fn walk_files(&self, root: &Path) -> io::Result<Vec<PathBuf>> {
        let root = root.to_path_buf();
        tokio::task::spawn_blocking(move || {
            Ok(WalkBuilder::new(root)
                .hidden(true)
                .git_ignore(true)
                .git_exclude(true)
                .build()
                .filter_map(Result::ok)
                .filter(|entry| entry.file_type().is_some_and(|kind| kind.is_file()))
                .map(|entry| entry.into_path())
                .collect())
        })
        .await
        .map_err(io::Error::other)?
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

#[derive(Debug, Clone)]
pub struct ProcessRequest {
    pub program: String,
    pub arguments: Vec<String>,
    pub working_directory: PathBuf,
    pub environment: HashMap<String, String>,
    pub timeout: Duration,
    pub network_mode: NetworkMode,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProcessStatus {
    Exited { exit_code: i32 },
    TimedOut,
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapturedOutput {
    head: Vec<u8>,
    tail: Vec<u8>,
    total_bytes: u64,
}

impl CapturedOutput {
    pub fn len(&self) -> usize {
        self.head.len() + self.tail.len()
    }

    pub fn is_empty(&self) -> bool {
        self.total_bytes == 0
    }

    pub fn total_bytes(&self) -> u64 {
        self.total_bytes
    }

    pub fn omitted_bytes(&self) -> u64 {
        self.total_bytes.saturating_sub(self.len() as u64)
    }

    pub fn is_truncated(&self) -> bool {
        self.omitted_bytes() > 0
    }

    pub fn render_lossy(&self) -> String {
        let mut rendered = String::from_utf8_lossy(&self.head).into_owned();
        let omitted = self.omitted_bytes();
        if omitted > 0 {
            rendered.push_str(&format!("\n... [{omitted} bytes omitted] ...\n"));
        }
        rendered.push_str(&String::from_utf8_lossy(&self.tail));
        rendered
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessOutput {
    pub stdout: CapturedOutput,
    pub stderr: CapturedOutput,
    pub status: ProcessStatus,
    pub elapsed: Duration,
    pub network_restriction_enforced: bool,
}

#[async_trait]
pub trait ProcessBackend: Send + Sync {
    async fn run(
        &self,
        request: ProcessRequest,
        call: &ToolCallContext,
    ) -> Result<ProcessOutput, ToolExecutionError>;
}

#[derive(Debug, Default)]
pub struct TokioProcessBackend;

#[async_trait]
impl ProcessBackend for TokioProcessBackend {
    async fn run(
        &self,
        request: ProcessRequest,
        call: &ToolCallContext,
    ) -> Result<ProcessOutput, ToolExecutionError> {
        let mut command = tokio::process::Command::new(&request.program);
        command
            .args(&request.arguments)
            .current_dir(&request.working_directory)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .env_clear()
            .envs(&request.environment);
        #[cfg(unix)]
        command.process_group(0);

        let started = Instant::now();
        let mut child = command.spawn().map_err(|error| {
            ToolExecutionError::execution(format!("failed to spawn command: {error}"))
        })?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| ToolExecutionError::execution("command stdout was not captured"))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| ToolExecutionError::execution("command stderr was not captured"))?;
        let stdout_task = tokio::spawn(read_bounded(stdout));
        let stderr_task = tokio::spawn(read_bounded(stderr));
        let timeout = effective_timeout(request.timeout, call.deadline);

        enum WaitOutcome {
            Completed(io::Result<std::process::ExitStatus>),
            Cancelled,
            TimedOut,
        }

        let outcome = tokio::select! {
            biased;
            _ = call.cancel.cancelled() => WaitOutcome::Cancelled,
            _ = tokio::time::sleep(timeout) => WaitOutcome::TimedOut,
            status = child.wait() => WaitOutcome::Completed(status),
        };

        let status = match outcome {
            WaitOutcome::Completed(Ok(status)) => ProcessStatus::Exited {
                exit_code: status.code().unwrap_or(-1),
            },
            WaitOutcome::Completed(Err(error)) => {
                let terminal_error =
                    ToolExecutionError::execution(format!("failed to wait for command: {error}"));
                let termination = terminate(&mut child).await;
                let _ = tokio::join!(
                    join_reader(stdout_task, "stdout"),
                    join_reader(stderr_task, "stderr")
                );
                termination?;
                return Err(terminal_error);
            }
            WaitOutcome::Cancelled => {
                terminate(&mut child).await?;
                ProcessStatus::Cancelled
            }
            WaitOutcome::TimedOut => {
                terminate(&mut child).await?;
                ProcessStatus::TimedOut
            }
        };

        let (stdout, stderr) = tokio::join!(
            join_reader(stdout_task, "stdout"),
            join_reader(stderr_task, "stderr")
        );
        Ok(ProcessOutput {
            stdout: stdout?,
            stderr: stderr?,
            status,
            elapsed: started.elapsed(),
            network_restriction_enforced: false,
        })
    }
}

async fn read_bounded(mut reader: impl tokio::io::AsyncRead + Unpin) -> io::Result<CapturedOutput> {
    let head_limit = MAX_CAPTURE_BYTES / 2;
    let tail_limit = MAX_CAPTURE_BYTES - head_limit;
    let mut head = Vec::with_capacity(head_limit);
    let mut tail = Vec::with_capacity(tail_limit);
    let mut total_bytes = 0u64;
    let mut buffer = [0u8; 8 * 1024];

    loop {
        let read = reader.read(&mut buffer).await?;
        if read == 0 {
            break;
        }
        total_bytes = total_bytes.saturating_add(read as u64);
        let mut chunk = &buffer[..read];
        if head.len() < head_limit {
            let keep = (head_limit - head.len()).min(chunk.len());
            head.extend_from_slice(&chunk[..keep]);
            chunk = &chunk[keep..];
        }
        if chunk.is_empty() {
            continue;
        }
        if chunk.len() >= tail_limit {
            tail.clear();
            tail.extend_from_slice(&chunk[chunk.len() - tail_limit..]);
            continue;
        }
        let overflow = tail
            .len()
            .saturating_add(chunk.len())
            .saturating_sub(tail_limit);
        if overflow > 0 {
            tail.drain(..overflow);
        }
        tail.extend_from_slice(chunk);
    }

    Ok(CapturedOutput {
        head,
        tail,
        total_bytes,
    })
}

async fn join_reader(
    task: tokio::task::JoinHandle<io::Result<CapturedOutput>>,
    stream: &str,
) -> Result<CapturedOutput, ToolExecutionError> {
    task.await
        .map_err(|error| {
            ToolExecutionError::execution(format!("command {stream} task failed: {error}"))
        })?
        .map_err(|error| {
            ToolExecutionError::execution(format!("failed to read command {stream}: {error}"))
        })
}

async fn terminate(child: &mut tokio::process::Child) -> Result<(), ToolExecutionError> {
    let running = child.try_wait().map_err(|error| {
        ToolExecutionError::outcome_unknown(format!("failed to inspect command state: {error}"))
    })?;
    if running.is_none() {
        terminate_running_process(child)?;
        child.wait().await.map_err(|error| {
            ToolExecutionError::outcome_unknown(format!(
                "failed to confirm command termination: {error}"
            ))
        })?;
    }
    Ok(())
}

#[cfg(unix)]
fn terminate_running_process(child: &mut tokio::process::Child) -> Result<(), ToolExecutionError> {
    let process_id = child
        .id()
        .ok_or_else(|| ToolExecutionError::outcome_unknown("running command has no process id"))?;
    let process_id = i32::try_from(process_id).map_err(|_| {
        ToolExecutionError::outcome_unknown("running command process id exceeds pid_t range")
    })?;
    // SAFETY: `process_id` comes from the live child created in its own process group.
    // Passing its negated value to `kill` targets that group and does not dereference memory.
    let result = unsafe { libc::kill(-process_id, libc::SIGKILL) };
    if result == 0 {
        return Ok(());
    }
    let error = io::Error::last_os_error();
    if error.raw_os_error() == Some(libc::ESRCH) {
        Ok(())
    } else {
        Err(ToolExecutionError::outcome_unknown(format!(
            "failed to terminate command process group: {error}"
        )))
    }
}

#[cfg(not(unix))]
fn terminate_running_process(child: &mut tokio::process::Child) -> Result<(), ToolExecutionError> {
    child.start_kill().map_err(|error| {
        ToolExecutionError::outcome_unknown(format!("failed to terminate command: {error}"))
    })
}

fn effective_timeout(timeout: Duration, deadline: Option<Instant>) -> Duration {
    deadline
        .map(|deadline| timeout.min(deadline.saturating_duration_since(Instant::now())))
        .unwrap_or(timeout)
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU64, Ordering};

    use tokio_util::sync::CancellationToken;

    use super::*;
    use crate::ToolCallId;

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

    #[cfg(unix)]
    #[tokio::test]
    async fn process_backend_terminates_a_cancelled_process_group() {
        let cancel = CancellationToken::new();
        let call = ToolCallContext::new(ToolCallId::new("cancel-test"), cancel.clone());
        let request = ProcessRequest {
            program: "sh".to_string(),
            arguments: vec!["-c".to_string(), "sleep 30 & wait".to_string()],
            working_directory: std::env::temp_dir(),
            environment: HashMap::new(),
            timeout: Duration::from_secs(60),
            network_mode: NetworkMode::Enabled,
        };
        let started = Instant::now();
        let task = tokio::spawn(async move { TokioProcessBackend.run(request, &call).await });
        tokio::time::sleep(Duration::from_millis(50)).await;
        cancel.cancel();

        let output = task
            .await
            .expect("backend task")
            .expect("cancelled command result");
        assert_eq!(output.status, ProcessStatus::Cancelled);
        assert!(started.elapsed() < Duration::from_secs(2));
    }

    #[tokio::test]
    async fn process_backend_enforces_timeout() {
        let call = ToolCallContext::new(ToolCallId::new("timeout-test"), CancellationToken::new());
        let request = ProcessRequest {
            program: "sh".to_string(),
            arguments: vec!["-c".to_string(), "sleep 30".to_string()],
            working_directory: std::env::temp_dir(),
            environment: HashMap::new(),
            timeout: Duration::from_millis(50),
            network_mode: NetworkMode::Enabled,
        };

        let output = TokioProcessBackend
            .run(request, &call)
            .await
            .expect("timed out command result");
        assert_eq!(output.status, ProcessStatus::TimedOut);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn process_backend_bounds_captured_output() {
        let call = ToolCallContext::new(ToolCallId::new("output-test"), CancellationToken::new());
        let request = ProcessRequest {
            program: "sh".to_string(),
            arguments: vec![
                "-c".to_string(),
                "printf HEAD; head -c 131072 /dev/zero; printf TAIL".to_string(),
            ],
            working_directory: std::env::temp_dir(),
            environment: HashMap::new(),
            timeout: Duration::from_secs(5),
            network_mode: NetworkMode::Enabled,
        };

        let output = TokioProcessBackend
            .run(request, &call)
            .await
            .expect("command output");

        assert!(output.stdout.len() <= 16 * 1024);
        assert!(output.stdout.is_truncated());
        assert_eq!(output.stdout.total_bytes(), 131_080);
        let rendered = output.stdout.render_lossy();
        assert!(rendered.starts_with("HEAD"));
        assert!(rendered.ends_with("TAIL"));
        assert!(rendered.contains("bytes omitted"));
    }
}
