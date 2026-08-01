use std::io;
use std::process::Stdio;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use tokio::io::AsyncReadExt;

use super::{CapturedOutput, ProcessBackend, ProcessOutput, ProcessRequest, ProcessStatus};
use crate::{ToolCallContext, ToolExecutionError, ToolProgress};

const MAX_CAPTURE_BYTES: usize = 16 * 1024;

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
        let stdout_call = call.clone();
        let stderr_call = call.clone();
        let stdout_task =
            tokio::spawn(
                async move { read_bounded(stdout, &stdout_call, OutputStream::Stdout).await },
            );
        let stderr_task =
            tokio::spawn(
                async move { read_bounded(stderr, &stderr_call, OutputStream::Stderr).await },
            );
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
        })
    }
}

#[derive(Debug, Clone, Copy)]
enum OutputStream {
    Stdout,
    Stderr,
}

async fn read_bounded(
    mut reader: impl tokio::io::AsyncRead + Unpin,
    call: &ToolCallContext,
    stream: OutputStream,
) -> io::Result<CapturedOutput> {
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
        let chunk = String::from_utf8_lossy(&buffer[..read]).into_owned();
        call.report_progress(match stream {
            OutputStream::Stdout => ToolProgress::Stdout { chunk },
            OutputStream::Stderr => ToolProgress::Stderr { chunk },
        });
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
    use std::collections::HashMap;

    use tokio_util::sync::CancellationToken;

    use super::*;
    use crate::ToolCallId;

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
