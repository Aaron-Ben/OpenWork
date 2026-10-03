//! Tool Span 的属性（trace.md、permissions.md §14.2）。
//!
//! 权限部分回答事故复盘时的两个问题：这条命令在什么约束下跑的？为什么问了 / 没问？

use openwork_sandbox::{Access, GrantScope, PathGrant};
use serde::{Deserialize, Serialize};

use super::trace::TRACE_SCHEMA_VERSION;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ToolTraceAttributesV1 {
    pub schema_version: u16,
    /// 这次调用执行时的模式；没有执行的调用（规则拒绝、沙箱不可用、用户拒绝）不记。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sandbox_mode: Option<String>,
    /// 调用发生时会话的模式。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_mode: Option<String>,
    /// `session_default` / `user_toggle` / `inherited`。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_mode_origin: Option<String>,
    /// 模型请求的精确路径越界。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub escalation_paths: Vec<EscalationPathTrace>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub escalation_justification: Option<String>,
    /// 命中的危险命令清单键，如 `rm_recursive_or_force`。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub danger_match: Option<String>,
    /// 是否被内核或文件工具围栏拒绝（permissions.md §7）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sandbox_denied: Option<bool>,
    /// `allow` / `ask` / `deny` / `cancelled`。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub permission_decision: Option<String>,
    /// `sandbox` / `user` / `builtin` / `non_interactive` / `sandbox_unavailable` / `system`。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub permission_decision_source: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub execution_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub artifact_count: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_retryable: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result_persisted: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_truncated: Option<bool>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub artifact_types: Vec<String>,
}

/// 一条越界路径，按 P3 的需要可以直接按路径聚合（permissions.md §14.2）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EscalationPathTrace {
    pub path: String,
    pub access: Access,
    pub scope: GrantScope,
}

impl From<&PathGrant> for EscalationPathTrace {
    fn from(grant: &PathGrant) -> Self {
        Self {
            path: grant.path.to_string_lossy().into_owned(),
            access: grant.access,
            scope: grant.scope,
        }
    }
}

impl ToolTraceAttributesV1 {
    pub fn new() -> Self {
        Self {
            schema_version: TRACE_SCHEMA_VERSION,
            sandbox_mode: None,
            session_mode: None,
            session_mode_origin: None,
            escalation_paths: Vec::new(),
            escalation_justification: None,
            danger_match: None,
            sandbox_denied: None,
            permission_decision: None,
            permission_decision_source: None,
            execution_ms: None,
            artifact_count: None,
            error_retryable: None,
            result_persisted: None,
            output_truncated: None,
            artifact_types: Vec::new(),
        }
    }
}

impl Default for ToolTraceAttributesV1 {
    fn default() -> Self {
        Self::new()
    }
}
