use serde::{Deserialize, Serialize};

use super::{Effect, ExecGrantSuggestion, RuleId};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AskSource {
    BuiltinSensitive,
    NoRuleCovers,
    Unparsed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DecisionSource {
    Builtin,
    SessionGrant,
    Mode,
    ModeFsCommand,
    ReadonlyProof,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum ApprovalSessionAction {
    AllowExec { grants: Vec<ExecGrantSuggestion> },
    EnableAcceptEdits,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "decision",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum UnitVerdict {
    Allow {
        source: DecisionSource,
        rule_id: Option<RuleId>,
    },
    Ask {
        source: AskSource,
        rule_id: Option<RuleId>,
    },
    Deny {
        rule_id: RuleId,
        silent: bool,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "certainty", rename_all = "snake_case")]
pub enum EffectDisplay {
    Inferred { effect: Effect },
    ReadonlyProof { key: String },
    TrustedProgram { program: String },
}

impl EffectDisplay {
    pub(crate) fn from_effect(effect: &Effect, readonly_proof_key: Option<&str>) -> Self {
        match effect {
            Effect::Exec { .. } if readonly_proof_key.is_some() => Self::ReadonlyProof {
                key: readonly_proof_key.expect("checked above").to_string(),
            },
            Effect::Exec { program, .. } => Self::TrustedProgram {
                program: program.clone(),
            },
            effect => Self::Inferred {
                effect: effect.clone(),
            },
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CardUnit {
    pub display: String,
    pub effects: Vec<EffectDisplay>,
    pub verdict: UnitVerdict,
    pub outside_workspace: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApprovalCard {
    pub units: Vec<CardUnit>,
    pub raw: String,
    pub unparsed: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_action: Option<ApprovalSessionAction>,
}
