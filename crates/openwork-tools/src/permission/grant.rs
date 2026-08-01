use serde::{Deserialize, Serialize};

use super::ExecPattern;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecGrantSuggestion {
    pub pattern: ExecPattern,
    pub label: String,
    pub exact: bool,
}

const ARITY_RULES: &[(&[&str], usize)] = &[
    (&["git", "config"], 3),
    (&["npm", "run"], 3),
    (&["pnpm", "run"], 3),
    (&["yarn", "run"], 3),
    (&["cargo"], 2),
    (&["git"], 2),
    (&["go"], 2),
];

/// Reduces one eligible exec subject to the session-sized grant described by
/// permissions.md §4.6.1. Eligibility is intentionally checked by the caller
/// before this function is used to offer an approval action.
pub fn reduce_exec_grant(program: &str, args: &[String]) -> ExecGrantSuggestion {
    let all_tokens = std::iter::once(program.to_string())
        .chain(args.iter().cloned())
        .collect::<Vec<_>>();
    let meaningful_tokens = all_tokens
        .iter()
        .filter(|token| !token.starts_with('-'))
        .cloned()
        .collect::<Vec<_>>();

    let arity = ARITY_RULES
        .iter()
        .filter(|(prefix, _)| {
            prefix.len() <= meaningful_tokens.len()
                && prefix
                    .iter()
                    .zip(meaningful_tokens.iter())
                    .all(|(expected, actual)| *expected == actual)
        })
        .max_by_key(|(prefix, _)| prefix.len())
        .map(|(_, arity)| *arity);

    if let Some(arity) = arity
        && meaningful_tokens.len() >= arity
    {
        let tokens = meaningful_tokens[..arity].to_vec();
        return ExecGrantSuggestion {
            label: tokens.join(" "),
            pattern: ExecPattern::TokenPrefix(tokens),
            exact: false,
        };
    }

    ExecGrantSuggestion {
        label: all_tokens.join(" "),
        pattern: ExecPattern::Literal(all_tokens),
        exact: true,
    }
}
