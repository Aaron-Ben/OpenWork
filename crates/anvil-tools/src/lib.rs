//! 工具执行层:Tool trait、ToolContext、审批策略层、ToolRegistry 与内置工具。

mod approval;
mod builtin;
mod registry;
mod tool;

pub use approval::{ApprovalBridge, ApprovalDecision, ApprovalPolicy, ApprovalsReviewer};
pub use registry::ToolRegistry;
pub use tool::{Tool, ToolContext, ToolError, ToolOutput};
