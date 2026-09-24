//! write / edit / bash 共用的越界参数（permissions.md §4.1–§4.2、tools.md 越界参数）。
//!
//! 这里只描述模型给的形状。路径规范化在 [`crate::ToolSessionContext`]，校验与批准在
//! Core（`SandboxPolicy::validate_grants` 加上 `justification` 非空）。

use openwork_sandbox::{Access, GrantScope, PathGrant};
use schemars::JsonSchema;
use serde::Deserialize;

/// 工具输入里用 `#[serde(flatten)]` 嵌入。沙箱不可用时两个字段会从 schema 里删掉。
#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct EscalationInput {
    /// Extra paths this one call needs beyond the current sandbox mode. Use it only to retry a
    /// call the sandbox denied, or when the call certainly needs them (e.g. `git push` reads
    /// ~/.ssh). The user is asked; nothing carries over to later calls.
    #[serde(default)]
    pub sandbox_permissions: Option<SandboxPermissionsInput>,
    /// One sentence shown to the user saying why this call needs those paths. Required with
    /// sandboxPermissions.
    #[serde(default)]
    pub justification: Option<String>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SandboxPermissionsInput {
    /// 1 to 16 entries, each only as wide as needed.
    pub paths: Vec<PathGrantInput>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PathGrantInput {
    /// Absolute path.
    pub path: String,
    /// `write` implies `read`.
    pub access: GrantAccessInput,
    /// `exact`: this file or directory only; `subtree`: a directory and everything under it.
    pub scope: GrantScopeInput,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum GrantAccessInput {
    Read,
    Write,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum GrantScopeInput {
    Exact,
    Subtree,
}

/// schema 里两个越界字段的名字：沙箱不可用时从 schema 中删掉（permissions.md §4.2）。
pub(crate) const ESCALATION_FIELDS: [&str; 2] = ["sandboxPermissions", "justification"];

/// Core 拿到的越界请求：路径已经规范化，理由原样保留（是否为空由 Core 校验）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Escalation {
    pub grants: Vec<PathGrant>,
    pub justification: String,
}

impl GrantAccessInput {
    pub(crate) fn access(self) -> Access {
        match self {
            Self::Read => Access::Read,
            Self::Write => Access::Write,
        }
    }
}

impl GrantScopeInput {
    pub(crate) fn scope(self) -> GrantScope {
        match self {
            Self::Exact => GrantScope::Exact,
            Self::Subtree => GrantScope::Subtree,
        }
    }
}
