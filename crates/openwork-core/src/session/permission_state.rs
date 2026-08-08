use std::sync::Arc;

use openwork_tools::{
    ExecGrantSuggestion, PermissionMode, Rule, RuleBehavior, RulePattern, RuleScope,
};
use serde::{Deserialize, Serialize};

use super::ToolCallId;

/// Whether anyone can answer an approval prompt for this Session.
///
/// **Not a third [`PermissionMode`].** A mode is an approval latitude the user
/// chose; this is a property of the runtime environment — whether a user exists
/// at all. Sub-agent Sessions run unattended, so an `Ask` there can never be
/// answered. See `docs/permissions.md` §6.6.
///
/// Everything else is unchanged: the same rule set, the same read-only proof,
/// the same built-in denials. Only the landing place of `Ask` differs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionApproval {
    /// A user is in the loop: `Ask` suspends the Tool Call and waits.
    #[default]
    Interactive,
    /// Nobody is in the loop: `Ask` is denied immediately.
    NonInteractive,
}

impl SessionApproval {
    pub fn is_interactive(self) -> bool {
        matches!(self, Self::Interactive)
    }
}

/// Returned to the model when an unattended Session hits `Ask`.
///
/// The wording has to be actionable. "Denied" alone makes the model retry the
/// same command until it burns through `max_model_calls`; naming the cause and
/// the way out lets it switch to a provably read-only command and carry on.
pub const NON_INTERACTIVE_DENIAL: &str = "This sub-agent runs unattended and cannot request approval. \
     Only provably read-only commands run without asking — for example \
     `git status`, `git log`, `git diff`, `rg`, `ls`, `cat`. \
     Re-run with one of those, or report what you could not determine.";

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
