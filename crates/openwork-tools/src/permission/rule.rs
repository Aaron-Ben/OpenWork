use std::path::Path;

use globset::{GlobBuilder, GlobMatcher};
use serde::{Deserialize, Serialize};

use super::Effect;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct RuleId(String);

impl RuleId {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuleBehavior {
    Allow,
    Ask,
    Deny,
}

impl RuleBehavior {
    pub(crate) fn severity(self) -> u8 {
        match self {
            Self::Allow => 0,
            Self::Ask => 1,
            Self::Deny => 2,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
/// Where a rule came from.
///
/// There are only two producers: the built-in rule set (code) and session
/// grants (a button the user pressed this session). permissions.md §3.6 rules
/// out a third — there is no permission config file — so this enum is complete,
/// not a stub awaiting more variants.
pub enum RuleScope {
    Builtin,
    Session,
}

impl RuleScope {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Builtin => "builtin",
            Self::Session => "session",
        }
    }
}

#[derive(Clone)]
pub struct PathPattern {
    source: String,
    matcher: GlobMatcher,
}

impl std::fmt::Debug for PathPattern {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_tuple("PathPattern")
            .field(&self.source)
            .finish()
    }
}

impl PathPattern {
    pub fn new(pattern: impl Into<String>) -> Result<Self, String> {
        let source = pattern.into();
        let glob = GlobBuilder::new(&source)
            .literal_separator(true)
            .backslash_escape(true)
            .build()
            .map_err(|error| error.to_string())?;
        Ok(Self {
            source,
            matcher: glob.compile_matcher(),
        })
    }

    pub fn is_match(&self, path: &Path) -> bool {
        self.matcher.is_match(path)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "tokens", rename_all = "snake_case")]
pub enum ExecPattern {
    TokenPrefix(Vec<String>),
    Literal(Vec<String>),
}

impl ExecPattern {
    fn is_match(&self, program: &str, args: &[String]) -> bool {
        let tokens = std::iter::once(program)
            .chain(args.iter().map(String::as_str))
            .collect::<Vec<_>>();
        match self {
            Self::TokenPrefix(pattern) => {
                pattern.len() <= tokens.len()
                    && pattern
                        .iter()
                        .zip(tokens.iter())
                        .all(|(expected, actual)| expected == actual)
            }
            Self::Literal(pattern) => {
                pattern.len() == tokens.len()
                    && pattern
                        .iter()
                        .zip(tokens.iter())
                        .all(|(expected, actual)| expected == actual)
            }
        }
    }
}

#[derive(Debug, Clone)]
pub enum RulePattern {
    Read(PathPattern),
    Write(PathPattern),
    Exec(ExecPattern),
}

impl RulePattern {
    pub(crate) fn is_match(&self, effect: &Effect) -> bool {
        match (self, effect) {
            (Self::Read(pattern), Effect::Read { path })
            | (Self::Write(pattern), Effect::Write { path }) => pattern.is_match(path),
            (Self::Exec(pattern), Effect::Exec { program, args }) => {
                pattern.is_match(program, args)
            }
            _ => false,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Rule {
    pub id: RuleId,
    pub pattern: RulePattern,
    pub behavior: RuleBehavior,
    pub scope: RuleScope,
    pub(crate) silent: bool,
    pub(crate) mode_only: bool,
}

impl Rule {
    pub fn new(
        id: impl Into<String>,
        pattern: RulePattern,
        behavior: RuleBehavior,
        scope: RuleScope,
    ) -> Self {
        Self {
            id: RuleId::new(id),
            pattern,
            behavior,
            scope,
            silent: false,
            mode_only: false,
        }
    }
}
