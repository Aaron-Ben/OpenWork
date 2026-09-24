use std::io;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::Mutex;

use super::{CapturedOutput, ProcessBackend, ProcessOutput, ProcessRequest, ProcessStatus};
use crate::spill::MAX_SPILL_BYTES;
use crate::{ToolCallContext, ToolExecutionError, ToolProgress};

/// Kept from the start of the output: the first error of a failing command is
/// sometimes here.
pub const CAPTURE_HEAD_BYTES: usize = 2 * 1024;
/// Kept from the end: compiler errors, test summaries and the final state.
pub const CAPTURE_TAIL_BYTES: usize = 14 * 1024;

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

    /// The complete output on disk, present only when something was omitted
    /// and the whole output was written successfully.
    pub fn spill_path(&self) -> Option<&std::path::Path> {
        self.spill_path.as_deref()
    }

    pub fn head_lossy(&self) -> String {
        String::from_utf8_lossy(&self.head).into_owned()
    }

    pub fn tail_lossy(&self) -> String {
        String::from_utf8_lossy(&self.tail).into_owned()
    }
}

/// Merged stdout/stderr capture shared by the two pipe readers.
struct Capture {
    head: Vec<u8>,
    tail: Vec<u8>,
    total_bytes: u64,
    full: FullOutput,
}

/// The complete output, kept only when the capture overflows.
enum FullOutput {
    /// Everything so far still fits in head + tail; no file yet.
    Buffering { path: PathBuf, bytes: Vec<u8> },
    Writing {
        path: PathBuf,
        file: tokio::fs::File,
    },
    /// The spill file reached [`MAX_SPILL_BYTES`]; it stays on disk, and the
    /// bounded text says it is incomplete.
    Capped { path: PathBuf },
    /// Spilling is disabled or failed; the capture never names a file.
    Unavailable,
}

impl Capture {
    fn new(spill_path: Option<PathBuf>) -> Self {
        Self {
            head: Vec::with_capacity(CAPTURE_HEAD_BYTES),
            tail: Vec::with_capacity(CAPTURE_TAIL_BYTES),
            total_bytes: 0,
            full: match spill_path {
                Some(path) => FullOutput::Buffering {
                    path,
                    bytes: Vec::new(),
                },
                None => FullOutput::Unavailable,
            },
        }
    }

    async fn push(&mut self, chunk: &[u8]) {
        self.total_bytes = self.total_bytes.saturating_add(chunk.len() as u64);
        self.keep_bounded(chunk);
        self.keep_full(chunk).await;
    }

    fn keep_bounded(&mut self, mut chunk: &[u8]) {
        if self.head.len() < CAPTURE_HEAD_BYTES {
            let keep = (CAPTURE_HEAD_BYTES - self.head.len()).min(chunk.len());
            self.head.extend_from_slice(&chunk[..keep]);
            chunk = &chunk[keep..];
        }
        if chunk.is_empty() {
            return;
        }
        if chunk.len() >= CAPTURE_TAIL_BYTES {
            self.tail.clear();
            self.tail
                .extend_from_slice(&chunk[chunk.len() - CAPTURE_TAIL_BYTES..]);
            return;
        }
        let overflow = (self.tail.len() + chunk.len()).saturating_sub(CAPTURE_TAIL_BYTES);
        if overflow > 0 {
            self.tail.drain(..overflow);
        }
        self.tail.extend_from_slice(chunk);
    }

    async fn keep_full(&mut self, chunk: &[u8]) {
        let full = std::mem::replace(&mut self.full, FullOutput::Unavailable);
        self.full = match full {
            FullOutput::Buffering { path, mut bytes } => {
                bytes.extend_from_slice(chunk);
                if bytes.len() <= CAPTURE_HEAD_BYTES + CAPTURE_TAIL_BYTES {
                    FullOutput::Buffering { path, bytes }
                } else {
                    start_spill(path, &bytes).await
                }
            }
            FullOutput::Writing { path, mut file } => {
                let written_before = self.total_bytes - chunk.len() as u64;
                let room = MAX_SPILL_BYTES.saturating_sub(written_before);
                let take = chunk.len().min(usize::try_from(room).unwrap_or(usize::MAX));
                if file.write_all(&chunk[..take]).await.is_err() {
                    abandon_spill(&path).await
                } else if take < chunk.len() {
                    let _ = file.flush().await;
                    FullOutput::Capped { path }
                } else {
                    FullOutput::Writing { path, file }
                }
            }
            other => other,
        };
    }

    /// The run failed and returns no output: a partial spill file would be
    /// named nowhere, so it is removed.
    async fn abandon(&mut self) {
        if let FullOutput::Writing { path, .. } | FullOutput::Capped { path } =
            std::mem::replace(&mut self.full, FullOutput::Unavailable)
        {
            let _ = tokio::fs::remove_file(&path).await;
        }
    }

    async fn finish(self) -> CapturedOutput {
        let spill_path = match self.full {
            FullOutput::Writing { path, mut file } => match file.flush().await {
                Ok(()) => Some(path),
                Err(_) => {
                    abandon_spill(&path).await;
                    None
                }
            },
            FullOutput::Capped { path } => Some(path),
            FullOutput::Buffering { .. } | FullOutput::Unavailable => None,
        };
        CapturedOutput {
            head: self.head,
            tail: self.tail,
            total_bytes: self.total_bytes,
            spill_path,
        }
    }
}

impl CapturedOutput {
    /// Whether the spill file stopped at [`MAX_SPILL_BYTES`].
    pub fn spill_is_capped(&self) -> bool {
        self.spill_path.is_some() && self.total_bytes > MAX_SPILL_BYTES
    }
}

async fn start_spill(path: PathBuf, bytes: &[u8]) -> FullOutput {
    if let Some(parent) = path.parent()
        && tokio::fs::create_dir_all(parent).await.is_err()
    {
        return FullOutput::Unavailable;
    }
    let Ok(mut file) = tokio::fs::File::create(&path).await else {
        return FullOutput::Unavailable;
    };
    if file.write_all(bytes).await.is_err() {
        return abandon_spill(&path).await;
    }
    FullOutput::Writing { path, file }
}

async fn abandon_spill(path: &std::path::Path) -> FullOutput {
    let _ = tokio::fs::remove_file(path).await;
    FullOutput::Unavailable
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
        let capture = Arc::new(Mutex::new(Capture::new(request.spill_path.clone())));
        let stdout_task = spawn_reader(stdout, call, OutputStream::Stdout, &capture);
        let stderr_task = spawn_reader(stderr, call, OutputStream::Stderr, &capture);
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
                capture.lock().await.abandon().await;
                termination?;
                return Err(terminal_error);
            }
            WaitOutcome::Cancelled => {
                if let Err(error) = terminate(&mut child).await {
                    capture.lock().await.abandon().await;
                    return Err(error);
                }
                ProcessStatus::Cancelled
            }
            WaitOutcome::TimedOut => {
                if let Err(error) = terminate(&mut child).await {
                    capture.lock().await.abandon().await;
                    return Err(error);
                }
                ProcessStatus::TimedOut
            }
        };

        let (stdout, stderr) = tokio::join!(
            join_reader(stdout_task, "stdout"),
            join_reader(stderr_task, "stderr")
        );
        if let Err(error) = stdout.and(stderr) {
            capture.lock().await.abandon().await;
            return Err(error);
        }
        let capture = Arc::try_unwrap(capture)
            .map_err(|_| ToolExecutionError::execution("command output is still being read"))?
            .into_inner();
        Ok(ProcessOutput {
            output: capture.finish().await,
            status,
            elapsed: started.elapsed(),
        })
    }
}

#[derive(Debug, Clone, Copy)]
enum OutputStream {
    Stdout,
    Stderr,
}

fn spawn_reader(
    reader: impl tokio::io::AsyncRead + Unpin + Send + 'static,
    call: &ToolCallContext,
    stream: OutputStream,
    capture: &Arc<Mutex<Capture>>,
) -> tokio::task::JoinHandle<io::Result<()>> {
    let call = call.clone();
    let capture = Arc::clone(capture);
    tokio::spawn(async move { read_into(reader, &call, stream, &capture).await })
}

/// Drains one pipe into the shared capture until EOF. Draining never stops
/// early: a full pipe would block the child.
async fn read_into(
    mut reader: impl tokio::io::AsyncRead + Unpin,
    call: &ToolCallContext,
    stream: OutputStream,
    capture: &Mutex<Capture>,
) -> io::Result<()> {
    let mut buffer = [0u8; 8 * 1024];
    loop {
        let read = reader.read(&mut buffer).await?;
        if read == 0 {
            return Ok(());
        }
        let chunk = String::from_utf8_lossy(&buffer[..read]).into_owned();
        call.report_progress(match stream {
            OutputStream::Stdout => ToolProgress::Stdout { chunk },
            OutputStream::Stderr => ToolProgress::Stderr { chunk },
        });
        capture.lock().await.push(&buffer[..read]).await;
    }
}

async fn join_reader(
    task: tokio::task::JoinHandle<io::Result<()>>,
    stream: &str,
) -> Result<(), ToolExecutionError> {
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
    use std::collections::HashMap;

    use tokio_util::sync::CancellationToken;

    use super::*;

    fn request(command: &str, timeout: Duration, spill_path: Option<PathBuf>) -> ProcessRequest {
        ProcessRequest {
            program: "sh".to_string(),
            arguments: vec!["-c".to_string(), command.to_string()],
            working_directory: std::env::temp_dir(),
            environment: HashMap::new(),
            timeout,
            spill_path,
        }
    }

    fn spill_path(label: &str) -> PathBuf {
        std::env::temp_dir()
            .join(format!("openwork-process-spill-{}", std::process::id()))
            .join(format!("{label}.txt"))
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn process_backend_terminates_a_cancelled_process_group() {
        let cancel = CancellationToken::new();
        let call = crate::test_support::call_context("cancel-test", cancel.clone());
        let request = request("sleep 30 & wait", Duration::from_secs(60), None);
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
        let call = crate::test_support::call_context("timeout-test", CancellationToken::new());
        let output = TokioProcessBackend
            .run(request("sleep 30", Duration::from_millis(50), None), &call)
            .await
            .expect("timed out command result");
        assert_eq!(output.status, ProcessStatus::TimedOut);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn keeps_a_short_head_and_a_long_tail() {
        let call = crate::test_support::call_context("output-test", CancellationToken::new());
        let output = TokioProcessBackend
            .run(
                request(
                    "printf HEAD; head -c 131072 /dev/zero; printf TAIL",
                    Duration::from_secs(5),
                    None,
                ),
                &call,
            )
            .await
            .expect("command output")
            .output;

        assert_eq!(output.len(), CAPTURE_HEAD_BYTES + CAPTURE_TAIL_BYTES);
        assert_eq!(output.total_bytes(), 131_080);
        assert!(output.head_lossy().starts_with("HEAD"));
        assert!(output.tail_lossy().ends_with("TAIL"));
        assert_eq!(output.spill_path(), None);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn merges_stdout_and_stderr() {
        let call = crate::test_support::call_context("merge-test", CancellationToken::new());
        let output = TokioProcessBackend
            .run(
                request(
                    "printf out; sleep 0.05; printf err >&2",
                    Duration::from_secs(5),
                    None,
                ),
                &call,
            )
            .await
            .expect("command output")
            .output;
        assert_eq!(output.head_lossy(), "outerr");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn spills_the_complete_output_only_when_it_overflows() {
        let call = crate::test_support::call_context("spill-test", CancellationToken::new());
        let small_path = spill_path("small");
        let small = TokioProcessBackend
            .run(
                request("seq 1 10", Duration::from_secs(5), Some(small_path.clone())),
                &call,
            )
            .await
            .expect("small output")
            .output;
        assert_eq!(small.spill_path(), None);
        assert!(!small_path.exists());

        let large_path = spill_path("large");
        let large = TokioProcessBackend
            .run(
                request(
                    "seq 1 20000",
                    Duration::from_secs(5),
                    Some(large_path.clone()),
                ),
                &call,
            )
            .await
            .expect("large output")
            .output;
        assert_eq!(large.spill_path(), Some(large_path.as_path()));
        let expected = (1..=20000).map(|n| format!("{n}\n")).collect::<String>();
        assert_eq!(
            std::fs::read_to_string(&large_path).expect("spill"),
            expected
        );
        let _ = std::fs::remove_file(large_path);
    }

    /// tools.md §12 #15: unbounded output neither exhausts memory nor fills
    /// the disk — the capture stays at head + tail and the spill file stops
    /// at its cap.
    #[cfg(unix)]
    #[tokio::test]
    async fn unbounded_output_stays_bounded_in_memory_and_on_disk() {
        let call = crate::test_support::call_context("yes-test", CancellationToken::new());
        let path = spill_path("yes");
        let output = TokioProcessBackend
            .run(
                request(
                    "head -c 70000000 /dev/zero",
                    Duration::from_secs(30),
                    Some(path.clone()),
                ),
                &call,
            )
            .await
            .expect("large output")
            .output;
        assert_eq!(output.len(), CAPTURE_HEAD_BYTES + CAPTURE_TAIL_BYTES);
        assert_eq!(output.total_bytes(), 70_000_000);
        assert!(output.spill_is_capped());
        assert_eq!(
            std::fs::metadata(&path).expect("spill").len(),
            MAX_SPILL_BYTES
        );
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn an_abandoned_capture_leaves_no_spill_file() {
        let path = spill_path("abandoned");
        let mut capture = Capture::new(Some(path.clone()));
        capture
            .push(&vec![b'x'; CAPTURE_HEAD_BYTES + CAPTURE_TAIL_BYTES + 1])
            .await;
        assert!(path.exists(), "overflow starts the spill file");

        capture.abandon().await;

        assert!(!path.exists());
        assert_eq!(capture.finish().await.spill_path(), None);
    }
}
