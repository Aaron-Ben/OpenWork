use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Weak};
use std::time::Instant;

use openwork_sandbox::{SandboxBackend, SandboxPolicy};
use tokio::sync::{Mutex, OwnedMutexGuard, mpsc};
use tokio_util::sync::CancellationToken;

use crate::backend::{AsyncFileSystem, LocalFileSystem, ProcessBackend, TokioProcessBackend};
use crate::checked_path::CheckedPath;
use crate::path::lexical_normalize;
use crate::{FileObservations, SpillDirectory, ToolProgress};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ToolCallId(String);

impl ToolCallId {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// 一次调用的上下文（tools.md §5）。
#[derive(Clone)]
pub struct ToolCallContext {
    pub call_id: ToolCallId,
    pub cancel: CancellationToken,
    pub deadline: Option<Instant>,
    /// 这一次调用生效的沙箱策略，由 Core 在调用前盖章：会话模式，加上用户为这一次批准的
    /// 越界路径。bash 的 Seatbelt profile 与文件工具的围栏读的是同一个值。
    pub sandbox_policy: SandboxPolicy,
    progress: Option<mpsc::Sender<ToolProgress>>,
}

impl ToolCallContext {
    pub fn new(
        call_id: ToolCallId,
        cancel: CancellationToken,
        sandbox_policy: SandboxPolicy,
    ) -> Self {
        Self {
            call_id,
            cancel,
            deadline: None,
            sandbox_policy,
            progress: None,
        }
    }

    pub fn with_deadline(mut self, deadline: Instant) -> Self {
        self.deadline = Some(deadline);
        self
    }

    pub fn with_progress_sender(mut self, progress: mpsc::Sender<ToolProgress>) -> Self {
        self.progress = Some(progress);
        self
    }

    /// Reports live progress without applying backpressure to tool execution.
    ///
    /// Full or disconnected channels intentionally drop the progress item. The
    /// terminal tool result remains the source of truth.
    pub fn report_progress(&self, progress: ToolProgress) {
        if let Some(sender) = &self.progress {
            let _ = sender.try_send(progress); // 进度是尽力而为的，丢一条不影响最终结果
        }
    }
}

/// 与 Session 同寿的上下文（tools.md §4）。不保存沙箱模式：模式在会话内会变，随调用走。
#[derive(Clone)]
pub struct ToolSessionContext {
    pub working_directory: PathBuf,
    /// 进程级的沙箱后端：持有启动自检的结论，把 bash 的 argv 包进 `sandbox-exec`。
    pub sandbox: Arc<dyn SandboxBackend>,
    pub environment: Arc<HashMap<String, String>>,
    pub filesystem: Arc<dyn AsyncFileSystem>,
    pub process_backend: Arc<dyn ProcessBackend>,
    /// Where bounded results keep their complete text. `None` disables
    /// spilling: results stay bounded but name no file.
    pub spill: Option<SpillDirectory>,
    /// What the model has read or written, for read-before-edit. Core shares
    /// one table across a Session's Turns.
    pub observations: FileObservations,
    write_locks: Arc<Mutex<HashMap<PathBuf, Weak<Mutex<()>>>>>,
}

impl ToolSessionContext {
    pub fn local(working_directory: PathBuf, sandbox: Arc<dyn SandboxBackend>) -> Self {
        Self::new(
            working_directory,
            sandbox,
            Arc::new(session_environment()),
            Arc::new(LocalFileSystem),
            Arc::new(TokioProcessBackend),
        )
    }

    pub fn new(
        working_directory: PathBuf,
        sandbox: Arc<dyn SandboxBackend>,
        environment: Arc<HashMap<String, String>>,
        filesystem: Arc<dyn AsyncFileSystem>,
        process_backend: Arc<dyn ProcessBackend>,
    ) -> Self {
        Self {
            working_directory,
            sandbox,
            environment,
            filesystem,
            process_backend,
            spill: None,
            observations: FileObservations::new(),
            write_locks: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn with_spill_directory(mut self, spill: SpillDirectory) -> Self {
        self.spill = Some(spill);
        self
    }

    pub fn with_file_observations(mut self, observations: FileObservations) -> Self {
        self.observations = observations;
        self
    }

    /// 沙箱可用时模型才能请求越界（permissions.md §4.2）。
    pub fn escalation_available(&self) -> bool {
        self.sandbox.status().is_available()
    }

    /// 模型给的路径按工作目录变成绝对路径，只做字面规范化。
    pub(crate) fn absolute(&self, input: &str) -> PathBuf {
        let requested = Path::new(input);
        let unresolved = if requested.is_absolute() {
            requested.to_path_buf()
        } else {
            self.working_directory.join(requested)
        };
        lexical_normalize(&unresolved)
    }

    pub(crate) async fn lock_for_write(&self, path: &CheckedPath) -> OwnedMutexGuard<()> {
        let path_lock = {
            let mut locks = self.write_locks.lock().await;
            if let Some(lock) = locks.get(path.as_path()).and_then(Weak::upgrade) {
                lock
            } else {
                let lock = Arc::new(Mutex::new(()));
                locks.insert(path.as_path().to_path_buf(), Arc::downgrade(&lock));
                lock
            }
        };
        path_lock.lock_owned().await
    }
}

fn session_environment() -> HashMap<String, String> {
    ["PATH", "HOME", "SHELL", "LANG", "LC_ALL", "TMPDIR"]
        .into_iter()
        .filter_map(|key| {
            std::env::var(key)
                .ok()
                .map(|value| (key.to_string(), value))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use tokio::sync::mpsc;
    use tokio_util::sync::CancellationToken;

    use super::*;
    use crate::ToolProgress;
    use crate::test_support::policy_for;

    fn call(progress: mpsc::Sender<ToolProgress>) -> ToolCallContext {
        ToolCallContext::new(
            ToolCallId::new("progress"),
            CancellationToken::new(),
            policy_for(&std::env::temp_dir()),
        )
        .with_progress_sender(progress)
    }

    #[tokio::test]
    async fn tool_call_context_reports_best_effort_progress() {
        let (progress_tx, mut progress_rx) = mpsc::channel(1);
        call(progress_tx).report_progress(ToolProgress::Message {
            message: "working".to_string(),
        });

        assert_eq!(
            progress_rx.recv().await,
            Some(ToolProgress::Message {
                message: "working".to_string()
            })
        );
    }

    #[test]
    fn disconnected_progress_consumer_does_not_fail_the_tool_call() {
        let (progress_tx, progress_rx) = mpsc::channel(1);
        drop(progress_rx);
        call(progress_tx).report_progress(ToolProgress::Message {
            message: "ignored".to_string(),
        });
    }
}
