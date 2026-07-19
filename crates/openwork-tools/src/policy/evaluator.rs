use serde::{Deserialize, Serialize};

pub(crate) fn evaluate(mode: PermissionMode, risk_hint: ToolRisk) -> PolicyDecision {
    match mode {
        PermissionMode::NeverAsk => PolicyDecision::Allow,
        PermissionMode::Ask => PolicyDecision::Ask {
            reason: approval_reason(risk_hint).to_string(),
        },
    }
}

fn approval_reason(risk_hint: ToolRisk) -> &'static str {
    match risk_hint {
        ToolRisk::ReadOnly => "read-only tool requires user permission",
        ToolRisk::WorkspaceMutation => "workspace mutation requires user permission",
        ToolRisk::ProcessExecution => "process execution requires user permission",
    }
}

use crate::ToolRisk;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PermissionMode {
    NeverAsk,
    Ask,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PolicyDecision {
    Allow,
    Ask { reason: String },
    Deny { reason: String },
}
