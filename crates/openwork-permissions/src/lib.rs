//! 权限与审批策略层。

mod approval;
mod permissions;

pub use approval::{ApprovalBridge, ApprovalDecision, ApprovalPolicy, ApprovalsReviewer};
pub use permissions::{
    AccessKind, FileSystemMode, FileSystemPermissions, NetworkMode, PermissionProfile,
};
