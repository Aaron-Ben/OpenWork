//! Tool definitions, validation, permission policy, and built-in execution.

mod backend;
mod builtins;
mod context;
mod definition;
mod file_change;
mod invocation;
mod observation;
mod permission;
pub mod policy;
mod progress;
mod registry;
mod result;
mod spill;
mod tool;

pub use backend::{
    AsyncFileSystem, AtomicWriteCondition, AtomicWriteError, AtomicWriteOutcome, CapturedOutput,
    FileSystemEntry, LocalFileSystem, ProcessBackend, ProcessOutput, ProcessRequest, ProcessStatus,
    TokioProcessBackend, WalkEntry,
};
pub use builtins::builtin_registry;
pub use context::{ToolCallContext, ToolCallId, ToolSessionContext};
pub use definition::{ToolDefinition, ToolId, ToolRisk};
pub use file_change::{
    FileChangeArtifact, FileChangeKind, FileChangeReapplyError, FileChangeUndoError, FileDiffHunk,
    FileDiffLine, FileDiffLineKind, ReapplyFileChangesResult, UndoFileChangesResult,
    reapply_file_changes, undo_file_changes,
};
pub use invocation::ToolInvocation;
pub use observation::FileObservations;
pub use permission::{
    AnalysisUnit, ApprovalCard, ApprovalSessionAction, AskSource, Authorization,
    AuthorizationEvidence, CardUnit, DecisionSource, Effect, EffectDisplay, ExecGrantSuggestion,
    ExecPattern, ExecutionPermit, InvocationAnalysis, PathPattern, PermissionEngine,
    PermissionMode, ReadonlyProof, Rule, RuleBehavior, RuleId, RulePattern, RuleScope, UnitVerdict,
    reduce_exec_grant,
};
pub use policy::{AccessKind, PermissionProfile};
pub use progress::ToolProgress;
pub use registry::{
    FinalizedToolset, ToolRegistryBuilder, ToolRegistryError, ToolValidationError, ToolsetConfig,
};
pub use result::{
    ToolError, ToolErrorCode, ToolExecutionError, ToolResult, ToolResultContent, ToolResultStatus,
};
pub use spill::{MAX_RESULT_BYTES, SpillDirectory};
pub use tool::{TextToolOutput, Tool, ToolOutput};
