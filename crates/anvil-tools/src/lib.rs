//! 工具执行层:Tool trait、ToolContext、Approval、ToolRegistry 与内置工具。

mod builtin;
mod registry;
mod tool;

pub use registry::ToolRegistry;
pub use tool::{
    AllowAllApproval, Approval, ApprovalDecision, Tool, ToolContext, ToolError, ToolOutput,
};
