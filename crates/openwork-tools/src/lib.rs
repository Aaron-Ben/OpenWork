//! Tool definitions, validation, permission policy, and built-in execution.

mod backend;
mod builtins;
mod checked_path;
mod context;
mod definition;
mod escalation;
mod file_change;
mod invocation;
mod notice;
mod observation;
mod path;
mod permission;
mod prepare;
mod progress;
mod registry;
mod result;
mod spill;
#[cfg(test)]
mod test_support;
mod tool;

pub use backend::{
    AsyncFileSystem, AtomicWriteCondition, AtomicWriteError, AtomicWriteOutcome, CapturedOutput,
    FileSystemEntry, LocalFileSystem, ProcessBackend, ProcessOutput, ProcessRequest, ProcessStatus,
    TokioProcessBackend, WalkEntry,
};
pub use builtins::builtin_registry;
pub use context::{ToolCallContext, ToolCallId, ToolSessionContext};
pub use definition::{ToolDefinition, ToolId, ToolRisk};
pub use escalation::{
    Escalation, EscalationInput, GrantAccessInput, GrantScopeInput, PathGrantInput,
    SandboxPermissionsInput,
};
pub use file_change::{
    FileChangeArtifact, FileChangeKind, FileChangeReapplyError, FileChangeUndoError, FileDiffHunk,
    FileDiffLine, FileDiffLineKind, ReapplyFileChangesResult, UndoFileChangesResult,
    reapply_file_changes, undo_file_changes,
};
pub use invocation::ToolInvocation;
pub use observation::FileObservations;
pub use permission::{DangerKey, DangerMatch, detect_danger};
pub use prepare::{CallInspection, PreparedCall};
pub use progress::ToolProgress;
pub use registry::{
    FinalizedToolset, ToolRegistryBuilder, ToolRegistryError, ToolValidationError, ToolsetConfig,
};
pub use result::{
    ToolError, ToolErrorCode, ToolExecutionError, ToolResult, ToolResultContent, ToolResultStatus,
};
pub use spill::{MAX_RESULT_BYTES, SpillDirectory};
pub use tool::{TextToolOutput, Tool, ToolOutput};
