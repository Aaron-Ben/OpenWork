use std::collections::HashMap;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use ignore::WalkBuilder;
use tokio::io::AsyncReadExt;

use crate::{ToolCallContext, ToolExecutionError};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileSystemEntry {
    pub name: String,
    pub is_directory: bool,
}

#[async_trait]
pub trait AsyncFileSystem: Send + Sync {
    async fn read_to_string(&self, path: &Path) -> io::Result<String>;
    async fn write(&self, path: &Path, content: &[u8]) -> io::Result<()>;
    async fn create_dir_all(&self, path: &Path) -> io::Result<()>;
    async fn exists(&self, path: &Path) -> io::Result<bool>;
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

    async fn write(&self, path: &Path, content: &[u8]) -> io::Result<()> {
        tokio::fs::write(path, content).await
    }

    async fn create_dir_all(&self, path: &Path) -> io::Result<()> {
        tokio::fs::create_dir_all(path).await
    }

    async fn exists(&self, path: &Path) -> io::Result<bool> {
        match tokio::fs::metadata(path).await {
            Ok(_) => Ok(true),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
            Err(error) => Err(error),
        }
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

#[derive(Debug, Clone)]
pub struct ProcessRequest {
    pub program: String,
    pub arguments: Vec<String>,
    pub working_directory: PathBuf,
    pub environment: HashMap<String, String>,
    pub timeout: Duration,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessOutput {
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub exit_code: i32,
    pub elapsed: Duration,
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
        let stdout_task = tokio::spawn(read_all(stdout));
        let stderr_task = tokio::spawn(read_all(stderr));
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

        let terminal_error = match outcome {
            WaitOutcome::Completed(Ok(status)) => {
                let (stdout, stderr) = tokio::join!(
                    join_reader(stdout_task, "stdout"),
                    join_reader(stderr_task, "stderr")
                );
                return Ok(ProcessOutput {
                    stdout: stdout?,
                    stderr: stderr?,
                    exit_code: status.code().unwrap_or(-1),
                    elapsed: started.elapsed(),
                });
            }
            WaitOutcome::Completed(Err(error)) => {
                ToolExecutionError::execution(format!("failed to wait for command: {error}"))
            }
            WaitOutcome::Cancelled => ToolExecutionError::cancelled("command cancelled"),
            WaitOutcome::TimedOut => ToolExecutionError::timeout(format!(
                "command timed out after {} ms",
                timeout.as_millis()
            )),
        };

        let termination = terminate(&mut child).await;
        let _ = tokio::join!(
            join_reader(stdout_task, "stdout"),
            join_reader(stderr_task, "stderr")
        );
        termination?;
        Err(terminal_error)
    }
}

async fn read_all(mut reader: impl tokio::io::AsyncRead + Unpin) -> io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    reader.read_to_end(&mut bytes).await?;
    Ok(bytes)
}

async fn join_reader(
    task: tokio::task::JoinHandle<io::Result<Vec<u8>>>,
    stream: &str,
) -> Result<Vec<u8>, ToolExecutionError> {
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
    use tokio_util::sync::CancellationToken;

    use super::*;
    use crate::{ToolCallId, ToolErrorCode};

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
        };
        let started = Instant::now();
        let task = tokio::spawn(async move { TokioProcessBackend.run(request, &call).await });
        tokio::time::sleep(Duration::from_millis(50)).await;
        cancel.cancel();

        let error = task
            .await
            .expect("backend task")
            .expect_err("cancelled command");
        assert_eq!(error.code, ToolErrorCode::Cancelled);
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
        };

        let error = TokioProcessBackend
            .run(request, &call)
            .await
            .expect_err("timed out command");
        assert_eq!(error.code, ToolErrorCode::Timeout);
    }
}
