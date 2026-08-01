use std::sync::Arc;

use openwork_tools::{
    ExecGrantSuggestion, PermissionMode, Rule, RuleBehavior, RulePattern, RuleScope,
};
use serde::{Deserialize, Serialize};

use super::ToolCallId;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PermissionModeOrigin {
    SessionDefault,
    UserToggle,
    ApprovalCard,
}

impl PermissionModeOrigin {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::SessionDefault => "session_default",
            Self::UserToggle => "user_toggle",
            Self::ApprovalCard => "approval_card",
        }
    }
}

#[derive(Clone)]
pub(super) struct SessionPermissionState {
    mode: PermissionMode,
    mode_origin: PermissionModeOrigin,
    session_rules: Arc<Vec<Rule>>,
}

impl SessionPermissionState {
    pub(super) fn new(mode: PermissionMode) -> Self {
        Self {
            mode,
            mode_origin: PermissionModeOrigin::SessionDefault,
            session_rules: Arc::new(Vec::new()),
        }
    }

    pub(super) fn mode(&self) -> PermissionMode {
        self.mode
    }

    pub(super) fn mode_origin(&self) -> PermissionModeOrigin {
        self.mode_origin
    }

    pub(super) fn session_rules(&self) -> &[Rule] {
        self.session_rules.as_slice()
    }

    pub(super) fn set_mode(&mut self, mode: PermissionMode, origin: PermissionModeOrigin) {
        self.mode = mode;
        self.mode_origin = origin;
    }

    pub(super) fn apply_exec_grants(
        &mut self,
        approval_tool_call_id: &ToolCallId,
        grants: &[ExecGrantSuggestion],
    ) {
        let mut rules = self.session_rules.as_ref().clone();
        rules.extend(grants.iter().enumerate().map(|(index, grant)| {
            Rule::new(
                format!("session.{}.{index}", approval_tool_call_id.as_str()),
                RulePattern::Exec(grant.pattern.clone()),
                RuleBehavior::Allow,
                RuleScope::Session,
            )
        }));
        self.session_rules = Arc::new(rules);
    }
}
