//! Seatbelt profile generation and `sandbox-exec` argv (permissions.md §3.1).
//!
//! The profile text never contains a path. Every path and every regex
//! reaches `sandbox-exec` as a `-D NAME=value` parameter that the text refers
//! to by name, so a directory called `x")(allow default` is just a string.

use std::collections::HashMap;
use std::path::Path;

use crate::policy::{Actor, GrantScope, PathGrant, SandboxPolicy};
use crate::tiers;

pub const SANDBOX_EXEC: &str = "/usr/bin/sandbox-exec";

/// A profile and the parameters its text refers to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SeatbeltProfile {
    pub text: String,
    pub parameters: Vec<(String, String)>,
}

impl SeatbeltProfile {
    /// Builds the profile that confines `actor` to `policy`. Only bash runs
    /// under Seatbelt today; the actor is explicit so tests can build the
    /// file-tool view of the same policy.
    pub fn new(policy: &SandboxPolicy, actor: Actor) -> Self {
        let mut builder = Builder::default();
        builder.line("(version 1)");
        builder.line("(allow default)");
        builder.line("(deny file-write*)");
        builder.allow_devices();
        for root in policy.base_writable_roots(actor) {
            let filter = builder.subpath(root);
            let carve_outs = builder.carve_outs(policy, true, true);
            builder.allow_write(&filter, &carve_outs);
        }
        for grant in policy.write_grants() {
            let filter = builder.grant(grant);
            let carve_outs = builder.carve_outs(
                policy,
                !policy.is_sensitive(&grant.path),
                !policy.is_credential(&grant.path),
            );
            builder.allow_write(&filter, &carve_outs);
        }
        for credential in tiers::credential_paths(policy.environment().home()) {
            let filter = builder.subpath(&credential);
            let exceptions = policy
                .path_grants
                .iter()
                .map(|grant| format!("(require-not {})", builder.grant(grant)))
                .collect::<Vec<_>>();
            if exceptions.is_empty() {
                builder.line(&format!("(deny file-read* {filter})"));
            } else {
                builder.line(&format!(
                    "(deny file-read* (require-all {filter} {}))",
                    exceptions.join(" ")
                ));
            }
        }
        builder.finish()
    }

    /// `sandbox-exec -p <text> -D ... -- <command...>`.
    pub fn wrap(&self, sandbox_exec: &Path, command: &[String]) -> Vec<String> {
        let mut argv = Vec::with_capacity(4 + self.parameters.len() * 2 + command.len());
        argv.push(sandbox_exec.to_string_lossy().into_owned());
        argv.push("-p".to_string());
        argv.push(self.text.clone());
        for (name, value) in &self.parameters {
            argv.push("-D".to_string());
            argv.push(format!("{name}={value}"));
        }
        argv.push("--".to_string());
        argv.extend(command.iter().cloned());
        argv
    }
}

#[derive(Default)]
struct Builder {
    text: String,
    parameters: Vec<(String, String)>,
    names: HashMap<String, String>,
}

impl Builder {
    fn line(&mut self, line: &str) {
        self.text.push_str(line);
        self.text.push('\n');
    }

    /// The same value always gets the same parameter name.
    fn param(&mut self, value: String) -> String {
        if let Some(name) = self.names.get(&value) {
            return format!("(param \"{name}\")");
        }
        let name = format!("P{}", self.parameters.len());
        self.names.insert(value.clone(), name.clone());
        self.parameters.push((name.clone(), value));
        format!("(param \"{name}\")")
    }

    fn subpath(&mut self, path: &Path) -> String {
        format!(
            "(subpath {})",
            self.param(path.to_string_lossy().into_owned())
        )
    }

    fn literal(&mut self, path: &Path) -> String {
        format!(
            "(literal {})",
            self.param(path.to_string_lossy().into_owned())
        )
    }

    fn regex(&mut self, regex: String) -> String {
        format!("(regex {})", self.param(regex))
    }

    fn grant(&mut self, grant: &PathGrant) -> String {
        match grant.scope {
            GrantScope::Exact => self.literal(&grant.path),
            GrantScope::Subtree => self.subpath(&grant.path),
        }
    }

    fn allow_devices(&mut self) {
        let mut filters = tiers::WRITABLE_DEVICES
            .iter()
            .map(|device| self.literal(Path::new(device)))
            .collect::<Vec<_>>();
        filters.extend(
            tiers::WRITABLE_DEVICE_REGEXES
                .iter()
                .map(|regex| self.regex((*regex).to_string())),
        );
        self.line(&format!("(allow file-write* {})", filters.join(" ")));
    }

    /// Hard-protected paths are carved out of every writable root and every
    /// grant; sensitive and credential paths out of everything that does not
    /// name them (permissions.md §3.1).
    fn carve_outs(
        &mut self,
        policy: &SandboxPolicy,
        sensitive: bool,
        credential: bool,
    ) -> Vec<String> {
        let mut filters = policy
            .environment()
            .hard_protected_roots()
            .map(Path::to_path_buf)
            .collect::<Vec<_>>()
            .iter()
            .map(|root| self.subpath(root))
            .collect::<Vec<_>>();
        filters.push(self.regex(tiers::workspace_hard_protected_regex(
            &policy.workspace_root,
        )));
        if sensitive {
            for regex in tiers::workspace_sensitive_regexes(&policy.workspace_root) {
                filters.push(self.regex(regex));
            }
        }
        if credential {
            for path in tiers::credential_paths(policy.environment().home()) {
                filters.push(self.subpath(&path));
            }
        }
        filters
            .into_iter()
            .map(|filter| format!("(require-not {filter})"))
            .collect()
    }

    fn allow_write(&mut self, filter: &str, carve_outs: &[String]) {
        self.line(&format!(
            "(allow file-write* (require-all {filter} {}))",
            carve_outs.join(" ")
        ));
    }

    fn finish(self) -> SeatbeltProfile {
        SeatbeltProfile {
            text: self.text,
            parameters: self.parameters,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::Arc;

    use super::*;
    use crate::policy::{Access, SandboxEnvironment, SandboxMode};

    fn policy(workspace: &str, mode: SandboxMode) -> SandboxPolicy {
        SandboxPolicy::new(
            mode,
            PathBuf::from(workspace),
            Arc::new(SandboxEnvironment::new(
                PathBuf::from("/home/me"),
                vec![PathBuf::from("/private/tmp")],
                vec![PathBuf::from("/home/me/.agents/skills")],
            )),
        )
    }

    #[test]
    fn hostile_paths_cannot_change_the_profile_text() {
        let hostile = r#"/home/me/we"ird (allow default) [x] {y} \z é"#;
        let plain = policy("/home/me/plain", SandboxMode::Auto).with_grants(vec![PathGrant {
            path: PathBuf::from("/home/me/plain/.git"),
            access: Access::Write,
            scope: GrantScope::Subtree,
        }]);
        let weird = policy(hostile, SandboxMode::Auto).with_grants(vec![PathGrant {
            path: PathBuf::from(format!("{hostile}/.git")),
            access: Access::Write,
            scope: GrantScope::Subtree,
        }]);

        let plain = SeatbeltProfile::new(&plain, Actor::Bash);
        let weird = SeatbeltProfile::new(&weird, Actor::Bash);

        assert_eq!(plain.text, weird.text, "only parameter values differ");
        assert!(!weird.text.contains("we\"ird"));
        assert!(weird.parameters.iter().any(|(_, value)| value == hostile));
        assert!(weird.parameters.iter().any(|(_, value)| {
            value.contains(r#"we"ird \(allow default\) \[x\] \{y\} \\z é/(.*/)?\.[gG][iI][tT]/[hH][oO][oO][kK][sS]"#)
        }));
    }

    #[test]
    fn accept_edits_leaves_the_workspace_out_of_bash_roots() {
        let workspace = "/home/me/project";
        let auto = SeatbeltProfile::new(&policy(workspace, SandboxMode::Auto), Actor::Bash);
        let accept =
            SeatbeltProfile::new(&policy(workspace, SandboxMode::AcceptEdits), Actor::Bash);
        let files = SeatbeltProfile::new(
            &policy(workspace, SandboxMode::AcceptEdits),
            Actor::FileTool,
        );
        let has_workspace = |profile: &SeatbeltProfile| {
            profile
                .parameters
                .iter()
                .any(|(_, value)| value == workspace)
        };
        assert!(has_workspace(&auto));
        assert!(!has_workspace(&accept));
        assert!(has_workspace(&files));
    }

    #[test]
    fn wrap_puts_parameters_before_the_command() {
        let profile = SeatbeltProfile {
            text: "(version 1)".to_string(),
            parameters: vec![("P0".to_string(), "/a=b".to_string())],
        };
        assert_eq!(
            profile.wrap(
                Path::new(SANDBOX_EXEC),
                &["bash".to_string(), "-c".to_string(), "ls".to_string()]
            ),
            [
                SANDBOX_EXEC,
                "-p",
                "(version 1)",
                "-D",
                "P0=/a=b",
                "--",
                "bash",
                "-c",
                "ls"
            ]
        );
    }
}
