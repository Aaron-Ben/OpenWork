//! Action 参数校验、安全执行与 Observation 归一化。

mod actions;
mod context;
mod handler;
mod invoker;
pub mod policy;
mod schema;
mod service;

pub use context::ExecutionContext;
pub use invoker::BuiltinActionInvoker;
pub use policy::{
    AccessKind, FileSystemMode, FileSystemPermissions, NetworkMode, PermissionProfile,
};
pub use service::ExecutionService;
