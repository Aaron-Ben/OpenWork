use std::path::{Path, PathBuf};

use globset::escape;

use super::{PathPattern, Rule, RuleBehavior, RulePattern, RuleScope};

#[derive(Debug, Clone)]
pub(crate) struct BuiltinRuleSet {
    workspace: PathBuf,
    rules: Vec<Rule>,
}

impl BuiltinRuleSet {
    pub(crate) fn for_workspace(workspace: impl Into<PathBuf>) -> Self {
        let workspace = workspace.into();
        let root = escape(workspace.to_string_lossy().as_ref());
        let mut rules = Vec::new();

        for name in [".git", ".openwork"] {
            for (suffix, label) in [
                (name.to_string(), "root"),
                (format!("{name}/**"), "tree"),
                (format!("**/{name}"), "nested"),
                (format!("**/{name}/**"), "nested_tree"),
            ] {
                let mut rule = Rule::new(
                    format!("builtin.deny.{name}.{label}"),
                    RulePattern::Write(path_pattern(format!("{root}/{suffix}"))),
                    RuleBehavior::Deny,
                    RuleScope::Builtin,
                );
                rule.silent = true;
                rules.push(rule);
            }
        }

        for (id, patterns) in [
            (
                "ide",
                vec![
                    ".vscode",
                    ".vscode/**",
                    ".idea",
                    ".idea/**",
                    "**/.vscode",
                    "**/.vscode/**",
                    "**/.idea",
                    "**/.idea/**",
                ],
            ),
            ("gitconfig", vec![".gitconfig", "**/.gitconfig"]),
            ("gitmodules", vec![".gitmodules", "**/.gitmodules"]),
            ("env", vec![".env*", "**/.env*"]),
            (
                "shell",
                vec![
                    ".bashrc",
                    ".bash_profile",
                    ".zshrc",
                    ".zprofile",
                    ".profile",
                    "**/.bashrc",
                    "**/.bash_profile",
                    "**/.zshrc",
                    "**/.zprofile",
                    "**/.profile",
                ],
            ),
        ] {
            for (index, suffix) in patterns.into_iter().enumerate() {
                rules.push(Rule::new(
                    format!("builtin.ask.{id}.{index}"),
                    RulePattern::Write(path_pattern(format!("{root}/{suffix}"))),
                    RuleBehavior::Ask,
                    RuleScope::Builtin,
                ));
            }
        }

        rules.push(Rule::new(
            "builtin.allow.workspace_root_read",
            RulePattern::Read(path_pattern(root.clone())),
            RuleBehavior::Allow,
            RuleScope::Builtin,
        ));
        rules.push(Rule::new(
            "builtin.allow.workspace_read",
            RulePattern::Read(path_pattern(format!("{root}/**"))),
            RuleBehavior::Allow,
            RuleScope::Builtin,
        ));
        let mut write = Rule::new(
            "builtin.allow.workspace_write",
            RulePattern::Write(path_pattern(format!("{root}/**"))),
            RuleBehavior::Allow,
            RuleScope::Builtin,
        );
        write.mode_only = true;
        rules.push(write);

        Self { workspace, rules }
    }

    pub(crate) fn workspace(&self) -> &Path {
        &self.workspace
    }

    pub(crate) fn rules(&self) -> &[Rule] {
        &self.rules
    }

    pub(crate) fn hard_deny_write(&self, path: &Path) -> bool {
        self.rules.iter().any(|rule| {
            rule.silent
                && rule.behavior == RuleBehavior::Deny
                && rule.pattern.is_match(&super::Effect::write(path))
        })
    }
}

fn path_pattern(pattern: String) -> PathPattern {
    PathPattern::new(pattern).expect("builtin permission glob must compile")
}
