use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::builtin::BuiltinRuleSet;
use super::{
    ApprovalCard, ApprovalSessionAction, AskSource, CardUnit, DecisionSource, Effect,
    EffectDisplay, ExecGrantSuggestion, ExecutionPermit, InvocationAnalysis, Rule, RuleBehavior,
    RuleId, RulePattern, RuleScope, UnitVerdict, reduce_exec_grant,
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

    pub fn authorize(
        &self,
        mode: PermissionMode,
        analysis: &InvocationAnalysis,
        session_rules: &[Rule],
    ) -> Authorization {
        self.authorize_internal(mode, analysis, session_rules, true)
    }

    fn authorize_internal(
        &self,
        mode: PermissionMode,
        analysis: &InvocationAnalysis,
        session_rules: &[Rule],
        include_session_action: bool,
    ) -> Authorization {
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
                self.authorize_unit(mode, unit, session_rules)
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
                session_action: include_session_action
                    .then(|| {
                        self.suggest_session_action(mode, analysis, &evaluated_units, session_rules)
                    })
                    .flatten(),
            },
            permit,
        }
    }

    fn authorize_unit(
        &self,
        mode: PermissionMode,
        unit: &super::AnalysisUnit,
        session_rules: &[Rule],
    ) -> EvaluatedUnit {
        if unit.effects.is_empty() {
            return EvaluatedUnit::ask(UnitVerdict::Ask {
                source: AskSource::NoRuleCovers,
                rule_id: None,
            });
        }

        let matched = unit
            .effects
            .iter()
            .map(|effect| self.authorize_effect(mode, effect, session_rules))
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

        let mut allow_evidence = Vec::with_capacity(unit.effects.len());
        for (effect, matched_rule) in unit.effects.iter().zip(matched.into_iter()) {
            if let Some(matched_rule) = matched_rule {
                allow_evidence.push(matched_rule);
                continue;
            }
            if matches!(effect, Effect::Exec { .. })
                && mode == PermissionMode::AcceptEdits
                && unit.filesystem_command_proof
            {
                allow_evidence.push(EvaluatedUnit {
                    verdict: UnitVerdict::Allow {
                        source: DecisionSource::ModeFsCommand,
                        rule_id: None,
                    },
                    rule_scope: None,
                    readonly_proof_key: None,
                });
                continue;
            }
            if matches!(effect, Effect::Exec { .. })
                && let Some(proof) = &unit.readonly_proof
            {
                allow_evidence.push(EvaluatedUnit {
                    verdict: UnitVerdict::Allow {
                        source: DecisionSource::ReadonlyProof,
                        rule_id: None,
                    },
                    rule_scope: None,
                    readonly_proof_key: Some(proof.key.clone()),
                });
                continue;
            }

            return EvaluatedUnit::ask(UnitVerdict::Ask {
                source: AskSource::NoRuleCovers,
                rule_id: None,
            });
        }

        let readonly_proof_used = allow_evidence.iter().any(|evidence| {
            verdict_decision_source(&evidence.verdict) == Some(&DecisionSource::ReadonlyProof)
        });
        let supporting_rule = allow_evidence
            .iter()
            .find(|evidence| verdict_rule_id(&evidence.verdict).is_some())
            .map(|evidence| (verdict_rule_id(&evidence.verdict), evidence.rule_scope));
        let mut deciding_evidence = allow_evidence
            .into_iter()
            .max_by_key(|result| allow_evidence_priority(&result.verdict))
            .expect("non-empty effects");
        if readonly_proof_used {
            deciding_evidence.readonly_proof_key =
                unit.readonly_proof.as_ref().map(|proof| proof.key.clone());
        }
        if matches!(
            deciding_evidence.verdict,
            UnitVerdict::Allow {
                source: DecisionSource::ReadonlyProof,
                rule_id: None,
            }
        ) && let Some((rule_id, rule_scope)) = supporting_rule
        {
            if let UnitVerdict::Allow {
                rule_id: deciding_rule_id,
                ..
            } = &mut deciding_evidence.verdict
            {
                *deciding_rule_id = rule_id;
            }
            deciding_evidence.rule_scope = rule_scope;
        }
        deciding_evidence
    }

    fn authorize_effect(
        &self,
        mode: PermissionMode,
        effect: &Effect,
        session_rules: &[Rule],
    ) -> Option<EvaluatedUnit> {
        let strongest = self
            .builtins
            .rules()
            .iter()
            .chain(self.additional_rules.iter())
            .chain(session_rules.iter())
            .filter(|rule| !(rule.mode_only && mode != PermissionMode::AcceptEdits))
            .filter(|rule| rule.pattern.is_match(effect))
            .max_by_key(|rule| rule.behavior.severity());

        if let Some(rule) = strongest {
            return Some(EvaluatedUnit {
                verdict: verdict_from_rule(rule),
                rule_scope: Some(rule.scope),
                readonly_proof_key: None,
            });
        }

        None
    }

    fn suggest_session_action(
        &self,
        mode: PermissionMode,
        analysis: &InvocationAnalysis,
        evaluated_units: &[EvaluatedUnit],
        session_rules: &[Rule],
    ) -> Option<ApprovalSessionAction> {
        if analysis.unparsed
            || analysis.units.iter().any(|unit| !unit.allow_eligible)
            || evaluated_units.iter().any(|unit| {
                matches!(
                    unit.verdict,
                    UnitVerdict::Ask {
                        source: AskSource::ExplicitRule | AskSource::BuiltinSensitive,
                        ..
                    }
                )
            })
        {
            return None;
        }

        let blocked = analysis
            .units
            .iter()
            .zip(evaluated_units.iter())
            .filter(|(_, evaluated)| matches!(evaluated.verdict, UnitVerdict::Ask { .. }))
            .map(|(unit, _)| unit)
            .collect::<Vec<_>>();
        if blocked.is_empty() {
            return None;
        }

        let mut grants = Vec::<ExecGrantSuggestion>::new();
        for unit in &blocked {
            let mut execs = unit.effects.iter().filter_map(|effect| match effect {
                Effect::Exec { program, args } => Some((program, args)),
                _ => None,
            });
            let Some((program, args)) = execs.next() else {
                grants.clear();
                break;
            };
            if execs.next().is_some() {
                grants.clear();
                break;
            }
            let grant = reduce_exec_grant(program, args);
            if !grants.iter().any(|existing| existing == &grant) {
                grants.push(grant);
            }
        }

        if !grants.is_empty() {
            let mut candidate_rules = session_rules.to_vec();
            candidate_rules.extend(grants.iter().enumerate().map(|(index, grant)| {
                Rule::new(
                    format!("session.suggestion.{index}"),
                    RulePattern::Exec(grant.pattern.clone()),
                    RuleBehavior::Allow,
                    RuleScope::Session,
                )
            }));
            if matches!(
                self.authorize_internal(mode, analysis, &candidate_rules, false),
                Authorization::Allow { .. }
            ) {
                return Some(ApprovalSessionAction::AllowExec { grants });
            }
        }

        if mode == PermissionMode::Default
            && matches!(
                self.authorize_internal(
                    PermissionMode::AcceptEdits,
                    analysis,
                    session_rules,
                    false,
                ),
                Authorization::Allow { .. }
            )
        {
            return Some(ApprovalSessionAction::EnableAcceptEdits);
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
    let source = units
        .iter()
        .filter_map(|unit| verdict_decision_source(&unit.verdict))
        .max_by_key(|source| decision_source_priority(source))
        .cloned()
        .expect("an allowed invocation has at least one allowed unit");
    let rule = units
        .iter()
        .find(|unit| {
            verdict_decision_source(&unit.verdict) == Some(&source)
                && verdict_rule_id(&unit.verdict).is_some()
        })
        .or_else(|| {
            units
                .iter()
                .find(|unit| verdict_rule_id(&unit.verdict).is_some())
        });
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

fn verdict_decision_source(verdict: &UnitVerdict) -> Option<&DecisionSource> {
    match verdict {
        UnitVerdict::Allow { source, .. } => Some(source),
        UnitVerdict::Ask { .. } | UnitVerdict::Deny { .. } => None,
    }
}

fn verdict_from_rule(rule: &Rule) -> UnitVerdict {
    match rule.behavior {
        RuleBehavior::Allow => UnitVerdict::Allow {
            source: if rule.mode_only {
                DecisionSource::Mode
            } else if rule.scope == RuleScope::Session {
                DecisionSource::SessionGrant
            } else if rule.scope == RuleScope::Builtin {
                DecisionSource::Builtin
            } else {
                DecisionSource::Rule
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

fn allow_evidence_priority(verdict: &UnitVerdict) -> u8 {
    match verdict {
        UnitVerdict::Allow { source, .. } => decision_source_priority(source),
        UnitVerdict::Ask { .. } | UnitVerdict::Deny { .. } => 0,
    }
}

fn decision_source_priority(source: &DecisionSource) -> u8 {
    match source {
        DecisionSource::ModeFsCommand => 6,
        DecisionSource::Mode => 5,
        DecisionSource::SessionGrant => 4,
        DecisionSource::ReadonlyProof => 3,
        DecisionSource::Rule => 2,
        DecisionSource::Builtin => 1,
    }
}
