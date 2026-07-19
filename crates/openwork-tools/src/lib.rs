//! Tool definitions, validation, permission policy, and built-in execution.

mod backend;
mod builtins;
mod context;
mod definition;
mod invocation;
pub mod policy;
mod registry;
mod result;
mod tool;

pub use backend::{
    AsyncFileSystem, AtomicWriteCondition, AtomicWriteError, AtomicWriteOutcome, CapturedOutput,
    FileSystemEntry, LocalFileSystem, ProcessBackend, ProcessOutput, ProcessRequest, ProcessStatus,
    TokioProcessBackend,
};
pub use builtins::builtin_registry;
pub use context::{ToolCallContext, ToolCallId, ToolSessionContext};
pub use definition::{ToolDefinition, ToolId, ToolRisk};
pub use invocation::ToolInvocation;
pub use policy::{
    AccessKind, FileSystemMode, FileSystemPermissions, NetworkMode, PermissionMode,
    PermissionProfile, PolicyDecision,
};
pub use registry::{
    FinalizedToolset, ToolRegistryBuilder, ToolRegistryError, ToolValidationError, ToolsetConfig,
};
pub use result::{
    ToolError, ToolErrorCode, ToolExecutionError, ToolResult, ToolResultContent, ToolResultStatus,
};
pub use tool::{TextToolOutput, Tool, ToolOutput};
