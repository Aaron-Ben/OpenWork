use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::builtin::BuiltinRuleSet;
use super::{
    ApprovalCard, AskSource, CardUnit, DecisionSource, Effect, EffectDisplay, ExecutionPermit,
    InvocationAnalysis, Rule, RuleBehavior, RuleId, UnitVerdict,
};
use crate::ToolErrorCode;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PermissionMode {
    Default,
    AcceptEdits,
}

impl Default for PermissionMode {
    fn default() -> Self {
        Self::Default
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Authorization {
    Allow {
        permit: ExecutionPermit,
    },
    Ask {
        card: ApprovalCard,
        permit: ExecutionPermit,
    },
    /// A rule refused the call (permissions.md §5.4 "规则拒绝").
    ///
    /// `silent` marks the built-in `.git` / `.openwork` write denials, whose
    /// decision source is `builtin` rather than a user-authored rule.
    Deny {
        reason: String,
        rule_id: RuleId,
        silent: bool,
    },
    /// The call could not be judged at all — unknown tool, malformed input, or
    /// a failure while extracting effects.
    ///
    /// This is **not** a permission verdict: reporting it as `Deny` would tell
    /// the model "a rule closed this path, try another approach" when the real
    /// problem is that the call itself was malformed (permissions.md §5.4).
    Unavailable {
        code: ToolErrorCode,
        message: String,
    },
}

pub struct PermissionEngine {
    builtins: BuiltinRuleSet,
    additional_rules: Vec<Rule>,
}

impl PermissionEngine {
    pub fn for_workspace(workspace: impl Into<PathBuf>) -> Self {
        Self {
            builtins: BuiltinRuleSet::for_workspace(workspace),
            additional_rules: Vec::new(),
        }
    }

    pub fn for_workspace_with_rules(
        workspace: impl Into<PathBuf>,
        additional_rules: Vec<Rule>,
    ) -> Self {
        Self {
            builtins: BuiltinRuleSet::for_workspace(workspace),
            additional_rules,
        }
    }

    pub fn authorize(&self, mode: PermissionMode, analysis: &InvocationAnalysis) -> Authorization {
        let permit = ExecutionPermit::new(analysis.effects().cloned().collect());
        let mut units = Vec::with_capacity(analysis.units.len());

        for unit in &analysis.units {
            let verdict = if analysis.unparsed {
                UnitVerdict::Ask {
                    source: AskSource::Unparsed,
                    rule_id: None,
                }
            } else {
                self.authorize_unit(mode, &unit.effects)
            };
            units.push(CardUnit {
                display: unit.display.clone(),
                effects: unit
                    .effects
                    .iter()
                    .map(EffectDisplay::from_effect)
                    .collect(),
                outside_workspace: unit.effects.iter().any(|effect| {
                    effect
                        .path()
                        .is_some_and(|path| !self.is_workspace_path(path))
                }),
                verdict,
            });
        }

        if let Some((rule_id, silent)) = units.iter().find_map(|unit| match &unit.verdict {
            UnitVerdict::Deny { rule_id, silent } => Some((rule_id.clone(), *silent)),
            _ => None,
        }) {
            return Authorization::Deny {
                reason: format!("permission denied by rule {}", rule_id.as_str()),
                rule_id,
                silent,
            };
        }

        if units
            .iter()
            .all(|unit| matches!(unit.verdict, UnitVerdict::Allow { .. }))
        {
            return Authorization::Allow { permit };
        }

        Authorization::Ask {
            card: ApprovalCard {
                units,
                raw: analysis.raw.clone(),
                unparsed: analysis.unparsed,
            },
            permit,
        }
    }

    fn authorize_unit(&self, mode: PermissionMode, effects: &[Effect]) -> UnitVerdict {
        if effects.is_empty() {
            return UnitVerdict::Ask {
                source: AskSource::NoRuleCovers,
                rule_id: None,
            };
        }

        let mut verdicts = effects
            .iter()
            .map(|effect| self.authorize_effect(mode, effect))
            .collect::<Vec<_>>();
        verdicts.sort_by_key(verdict_severity);
        verdicts.pop().expect("non-empty effects")
    }

    fn authorize_effect(&self, mode: PermissionMode, effect: &Effect) -> UnitVerdict {
        let strongest = self
            .builtins
            .rules()
            .iter()
            .chain(self.additional_rules.iter())
            .filter(|rule| !(rule.mode_only && mode != PermissionMode::AcceptEdits))
            .filter(|rule| rule.pattern.is_match(effect))
            .max_by_key(|rule| rule.behavior.severity());

        if let Some(rule) = strongest {
            if matches!(effect, Effect::Exec { .. }) && rule.behavior == RuleBehavior::Allow {
                return UnitVerdict::Ask {
                    source: AskSource::NoRuleCovers,
                    rule_id: None,
                };
            }
            return verdict_from_rule(rule);
        }

        UnitVerdict::Ask {
            source: AskSource::NoRuleCovers,
            rule_id: None,
        }
    }

    fn is_workspace_path(&self, path: &Path) -> bool {
        path.starts_with(self.builtins.workspace())
    }
}

fn verdict_from_rule(rule: &Rule) -> UnitVerdict {
    match rule.behavior {
        RuleBehavior::Allow => UnitVerdict::Allow {
            source: if rule.mode_only {
                DecisionSource::Mode
            } else {
                DecisionSource::Builtin
            },
            rule_id: Some(rule.id.clone()),
        },
        RuleBehavior::Ask => UnitVerdict::Ask {
            source: if rule.sensitive {
                AskSource::BuiltinSensitive
            } else {
                AskSource::ExplicitRule
            },
            rule_id: Some(rule.id.clone()),
        },
        RuleBehavior::Deny => UnitVerdict::Deny {
            rule_id: rule.id.clone(),
            silent: rule.silent,
        },
    }
}

fn verdict_severity(verdict: &UnitVerdict) -> u8 {
    match verdict {
        UnitVerdict::Allow { .. } => 0,
        UnitVerdict::Ask { .. } => 1,
        UnitVerdict::Deny { .. } => 2,
    }
}
