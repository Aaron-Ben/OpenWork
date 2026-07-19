//! Tool definitions, validation, permission policy, and built-in execution.

mod backend;
mod builtins;
mod context;
mod definition;
mod file_change;
mod invocation;
pub mod policy;
mod progress;
mod registry;
mod result;
mod tool;

pub use backend::{
    AsyncFileSystem, AtomicWriteCondition, AtomicWriteError, AtomicWriteOutcome, CapturedOutput,
    FileSystemEntry, FileWalk, LocalFileSystem, ProcessBackend, ProcessOutput, ProcessRequest,
    ProcessStatus, TokioProcessBackend,
};
pub use builtins::builtin_registry;
pub use context::{ToolCallContext, ToolCallId, ToolSessionContext};
pub use definition::{ToolDefinition, ToolId, ToolRisk};
pub use file_change::{
    FileChangeArtifact, FileChangeKind, FileChangeUndoError, FileDiffHunk, FileDiffLine,
    FileDiffLineKind, UndoFileChangesResult, undo_file_changes,
};
pub use invocation::ToolInvocation;
pub use policy::{
    AccessKind, FileSystemMode, FileSystemPermissions, NetworkMode, PermissionMode,
    PermissionProfile, PolicyDecision,
};
pub use progress::ToolProgress;
pub use registry::{
    FinalizedToolset, ToolRegistryBuilder, ToolRegistryError, ToolValidationError, ToolsetConfig,
};
pub use result::{
    ToolError, ToolErrorCode, ToolExecutionError, ToolResult, ToolResultContent, ToolResultStatus,
};
pub use tool::{TextToolOutput, Tool, ToolOutput};
