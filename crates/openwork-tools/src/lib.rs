//! Tool definitions, validation, permission policy, and built-in execution.

mod builtins;
mod catalog;
mod context;
mod definition;
mod executor;
mod handler;
mod invocation;
pub mod policy;
mod result;
mod schema;

pub use catalog::{CatalogError, ToolCatalog, ToolValidationError};
pub use context::ToolContext;
pub use definition::{ToolDefinition, ToolRisk};
pub use executor::{BuiltinToolExecutor, ToolExecutor};
pub use invocation::ToolInvocation;
pub use policy::{
    AccessKind, FileSystemMode, FileSystemPermissions, NetworkMode, PermissionMode,
    PermissionProfile, PolicyDecision,
};
pub use result::{ToolError, ToolErrorCode, ToolResult, ToolResultContent, ToolResultStatus};

pub(crate) use context::ToolContext as ExecutionContext;
pub(crate) use result::{ToolErrorCode as ObservationErrorCode, ToolResult as Observation};
