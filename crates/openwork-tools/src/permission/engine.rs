use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::builtin::BuiltinRuleSet;
use super::{
    ApprovalCard, AskSource, CardUnit, DecisionSource, Effect, EffectDisplay, ExecutionPermit,
    InvocationAnalysis, Rule, RuleBehavior, RuleId, RuleScope, UnitVerdict,
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
        evidence: AuthorizationEvidence,
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
        rule_scope: RuleScope,
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthorizationEvidence {
    pub source: DecisionSource,
    pub rule_id: Option<RuleId>,
    pub rule_scope: Option<RuleScope>,
    pub readonly_proof_key: Option<String>,
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
        let mut evaluated_units = Vec::with_capacity(analysis.units.len());

        for unit in &analysis.units {
            let evaluated = if analysis.unparsed {
                EvaluatedUnit::ask(UnitVerdict::Ask {
                    source: AskSource::Unparsed,
                    rule_id: None,
                })
            } else {
                self.authorize_unit(mode, unit)
            };
            units.push(CardUnit {
                display: unit.display.clone(),
                effects: unit
                    .effects
                    .iter()
                    .map(|effect| {
                        EffectDisplay::from_effect(
                            effect,
                            unit.readonly_proof.as_ref().map(|proof| proof.key.as_str()),
                        )
                    })
                    .collect(),
                outside_workspace: unit.effects.iter().any(|effect| {
                    effect
                        .path()
                        .is_some_and(|path| !self.is_workspace_path(path))
                }),
                verdict: evaluated.verdict.clone(),
            });
            evaluated_units.push(evaluated);
        }

        if let Some(evaluated) = evaluated_units
            .iter()
            .find(|unit| matches!(unit.verdict, UnitVerdict::Deny { .. }))
        {
            let UnitVerdict::Deny { rule_id, silent } = &evaluated.verdict else {
                unreachable!("filtered to deny")
            };
            return Authorization::Deny {
                reason: format!("permission denied by rule {}", rule_id.as_str()),
                rule_id: rule_id.clone(),
                rule_scope: evaluated
                    .rule_scope
                    .expect("rule denials always carry their scope"),
                silent: *silent,
            };
        }

        if evaluated_units
            .iter()
            .all(|unit| matches!(unit.verdict, UnitVerdict::Allow { .. }))
        {
            return Authorization::Allow {
                permit,
                evidence: aggregate_allow_evidence(&evaluated_units),
            };
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

    fn authorize_unit(&self, mode: PermissionMode, unit: &super::AnalysisUnit) -> EvaluatedUnit {
        if unit.effects.is_empty() {
            return EvaluatedUnit::ask(UnitVerdict::Ask {
                source: AskSource::NoRuleCovers,
                rule_id: None,
            });
        }

        let matched = unit
            .effects
            .iter()
            .map(|effect| self.authorize_effect(mode, effect))
            .collect::<Vec<_>>();

        if let Some(strongest) = matched
            .iter()
            .flatten()
            .max_by_key(|result| verdict_severity(&result.verdict))
            .filter(|result| !matches!(result.verdict, UnitVerdict::Allow { .. }))
        {
            return strongest.clone();
        }

        let has_exec = unit
            .effects
            .iter()
            .any(|effect| matches!(effect, Effect::Exec { .. }));
        if has_exec && !unit.allow_eligible {
            return EvaluatedUnit::ask(UnitVerdict::Ask {
                source: AskSource::NoRuleCovers,
                rule_id: None,
            });
        }

        if has_exec && unit.readonly_proof.is_some() {
            if unit
                .effects
                .iter()
                .any(|effect| matches!(effect, Effect::Write { .. }))
            {
                return EvaluatedUnit::ask(UnitVerdict::Ask {
                    source: AskSource::NoRuleCovers,
                    rule_id: None,
                });
            }
            let all_reads_allowed =
                unit.effects
                    .iter()
                    .zip(matched.iter())
                    .all(|(effect, result)| match effect {
                        Effect::Exec { .. } => result.is_none(),
                        Effect::Read { .. } => result.as_ref().is_some_and(|result| {
                            matches!(result.verdict, UnitVerdict::Allow { .. })
                        }),
                        Effect::Write { .. } => false,
                    });
            if all_reads_allowed {
                let supporting_rule = matched.iter().flatten().next();
                return EvaluatedUnit {
                    verdict: UnitVerdict::Allow {
                        source: DecisionSource::ReadonlyProof,
                        rule_id: supporting_rule
                            .as_ref()
                            .and_then(|result| verdict_rule_id(&result.verdict)),
                    },
                    rule_scope: supporting_rule.and_then(|result| result.rule_scope),
                    readonly_proof_key: unit.readonly_proof.as_ref().map(|proof| proof.key.clone()),
                };
            }
        }

        if matched.iter().all(Option::is_some) {
            return matched
                .into_iter()
                .flatten()
                .max_by_key(|result| verdict_severity(&result.verdict))
                .expect("non-empty effects");
        }

        EvaluatedUnit::ask(UnitVerdict::Ask {
            source: AskSource::NoRuleCovers,
            rule_id: None,
        })
    }

    fn authorize_effect(&self, mode: PermissionMode, effect: &Effect) -> Option<EvaluatedUnit> {
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
                return Some(EvaluatedUnit::ask(UnitVerdict::Ask {
                    source: AskSource::NoRuleCovers,
                    rule_id: None,
                }));
            }
            return Some(EvaluatedUnit {
                verdict: verdict_from_rule(rule),
                rule_scope: Some(rule.scope),
                readonly_proof_key: None,
            });
        }

        None
    }

    fn is_workspace_path(&self, path: &Path) -> bool {
        path.starts_with(self.builtins.workspace())
    }
}

#[derive(Debug, Clone)]
struct EvaluatedUnit {
    verdict: UnitVerdict,
    rule_scope: Option<RuleScope>,
    readonly_proof_key: Option<String>,
}

impl EvaluatedUnit {
    fn ask(verdict: UnitVerdict) -> Self {
        Self {
            verdict,
            rule_scope: None,
            readonly_proof_key: None,
        }
    }
}

fn aggregate_allow_evidence(units: &[EvaluatedUnit]) -> AuthorizationEvidence {
    let source = if units.iter().any(|unit| {
        matches!(
            unit.verdict,
            UnitVerdict::Allow {
                source: DecisionSource::ReadonlyProof,
                ..
            }
        )
    }) {
        DecisionSource::ReadonlyProof
    } else if units.iter().any(|unit| {
        matches!(
            unit.verdict,
            UnitVerdict::Allow {
                source: DecisionSource::Mode,
                ..
            }
        )
    }) {
        DecisionSource::Mode
    } else {
        DecisionSource::Builtin
    };
    let rule = units
        .iter()
        .find(|unit| verdict_rule_id(&unit.verdict).is_some());
    let mut proof_keys = units
        .iter()
        .filter_map(|unit| unit.readonly_proof_key.as_deref())
        .collect::<Vec<_>>();
    proof_keys.sort_unstable();
    proof_keys.dedup();

    AuthorizationEvidence {
        source,
        rule_id: rule.and_then(|unit| verdict_rule_id(&unit.verdict)),
        rule_scope: rule.and_then(|unit| unit.rule_scope),
        // A Tool Call may contain multiple independently proved shell units.
        // Single-unit calls retain the exact table key; compound calls keep a
        // stable comma-separated set so every contributing key remains
        // searchable from the one string field defined by permissions.md §7.
        readonly_proof_key: (!proof_keys.is_empty()).then(|| proof_keys.join(",")),
    }
}

fn verdict_rule_id(verdict: &UnitVerdict) -> Option<RuleId> {
    match verdict {
        UnitVerdict::Allow { rule_id, .. } | UnitVerdict::Ask { rule_id, .. } => rule_id.clone(),
        UnitVerdict::Deny { rule_id, .. } => Some(rule_id.clone()),
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
