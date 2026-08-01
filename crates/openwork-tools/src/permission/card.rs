use serde::{Deserialize, Serialize};

use super::{Effect, RuleId};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AskSource {
    ExplicitRule,
    BuiltinSensitive,
    NoRuleCovers,
    Unparsed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DecisionSource {
    Builtin,
    Mode,
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
    TrustedProgram { program: String },
}

impl EffectDisplay {
    pub(crate) fn from_effect(effect: &Effect) -> Self {
        match effect {
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
}
