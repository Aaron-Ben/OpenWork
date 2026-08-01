use std::collections::HashMap;
use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

use async_trait::async_trait;
use thiserror::Error;

use crate::{ToolCallContext, ToolExecutionError};

mod filesystem;
mod process;
mod walk;

pub use filesystem::LocalFileSystem;
pub use process::TokioProcessBackend;
pub use walk::FileWalk;

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
    async fn remove_file_if_matches(
        &self,
        path: &Path,
        expected: &[u8],
    ) -> Result<(), AtomicWriteError>;
    async fn create_dir_all(&self, path: &Path) -> io::Result<()>;
    async fn canonicalize(&self, path: &Path) -> io::Result<PathBuf>;
    async fn is_symlink(&self, path: &Path) -> io::Result<bool>;
    async fn read_dir(&self, path: &Path) -> io::Result<Vec<FileSystemEntry>>;
    async fn walk_files(&self, root: &Path) -> io::Result<FileWalk>;
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessOutput {
    pub stdout: CapturedOutput,
    pub stderr: CapturedOutput,
    pub status: ProcessStatus,
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
