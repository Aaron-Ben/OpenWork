//! 生成 Seatbelt profile 与 `sandbox-exec` 的 argv（permissions.md §5）。
//!
//! profile 正文里从不出现路径。每个路径和正则都以 `-D NAME=value` 参数传给 `sandbox-exec`，
//! 正文只按名字引用，因此名叫 `x")(allow default` 的目录也只是一个字符串。

use std::collections::HashMap;
use std::path::Path;

use crate::confinement::EngineConfinement;
use crate::policy::{Actor, GrantScope, PathGrant, SandboxPolicy};
use crate::tiers;

pub const SANDBOX_EXEC: &str = "/usr/bin/sandbox-exec";

/// 一份 profile，以及它的正文引用的参数。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SeatbeltProfile {
    pub text: String,
    pub parameters: Vec<(String, String)>,
}

impl SeatbeltProfile {
    /// 生成把 `actor` 约束在 `policy` 内的 profile。目前只有 bash 在 Seatbelt 下运行；
    /// actor 显式传入，是为了测试能生成同一策略下文件工具的视角。
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

    /// 生成把一个协作 Agent 的 Engine 进程树关进 `confinement` 的 profile（collaboration.md §3.1）。
    ///
    /// `$HOME` 之内只拒绝 `file-read-data`（读内容、列目录），不拒绝 `stat`：解析 Agent
    /// 目录的上级路径、`realpath` 都要读上级目录的元数据。
    pub fn confined(confinement: &EngineConfinement) -> Self {
        let mut builder = Builder::default();
        builder.line("(version 1)");
        builder.line("(allow default)");
        builder.line("(deny file-write*)");
        builder.allow_devices();
        let writable = confinement
            .writable_roots()
            .map(Path::to_path_buf)
            .collect::<Vec<_>>()
            .iter()
            .map(|root| builder.subpath(root))
            .collect::<Vec<_>>();
        if !writable.is_empty() {
            builder.line(&format!("(allow file-write* {})", writable.join(" ")));
        }
        let home = builder.subpath(confinement.home());
        let exceptions = confinement
            .home_read_exceptions()
            .map(Path::to_path_buf)
            .collect::<Vec<_>>()
            .iter()
            .map(|path| format!("(require-not {})", builder.subpath(path)))
            .collect::<Vec<_>>();
        builder.line(&format!(
            "(deny file-read-data (require-all {home}{}{}))",
            if exceptions.is_empty() { "" } else { " " },
            exceptions.join(" ")
        ));
        builder.finish()
    }

    /// `sandbox-exec -p <正文> -D ... -- <命令...>`。
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

    /// 同一个值总是得到同一个参数名。
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

    /// 硬保护路径从每个可写根和每条授权中扣除；敏感与凭据路径从所有没有点名它们的
    /// 可写范围中扣除（permissions.md §5）。
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

    /// collaboration.md §3.1：写只放行临时根与 Agent 根；`$HOME` 内只拒绝读内容，例外逐条扣除；
    /// 路径只经 `-D` 参数进入。
    #[test]
    fn engine_confinement_profile_names_every_path_only_as_a_parameter() {
        let environment = SandboxEnvironment::new(
            PathBuf::from("/home/me"),
            vec![PathBuf::from("/private/tmp")],
            Vec::new(),
        );
        let agent = r#"/home/me/.openwork/agents/a"b (allow default)"#;
        let confinement = EngineConfinement::new(&environment)
            .with_writable_root(Path::new(agent))
            .with_readable_path(Path::new(
                "/home/me/.openwork/runtime/s/agents/a/runtime-token",
            ));

        let profile = SeatbeltProfile::confined(&confinement);

        assert!(!profile.text.contains("/home/me"));
        assert!(!profile.text.contains("/private/tmp"));
        let name = |value: &str| {
            let (name, _) = profile
                .parameters
                .iter()
                .find(|(_, parameter)| parameter == value)
                .unwrap_or_else(|| panic!("missing parameter {value}"));
            name.clone()
        };
        let (home, temp, root, token) = (
            name("/home/me"),
            name("/private/tmp"),
            name(agent),
            name("/home/me/.openwork/runtime/s/agents/a/runtime-token"),
        );
        assert!(profile.text.contains(&format!(
            "(allow file-write* (subpath (param \"{temp}\")) (subpath (param \"{root}\")))"
        )));
        assert!(profile.text.contains(&format!(
            "(deny file-read-data (require-all (subpath (param \"{home}\")) \
             (require-not (subpath (param \"{temp}\"))) \
             (require-not (subpath (param \"{root}\"))) \
             (require-not (subpath (param \"{token}\")))))"
        )));
        assert!(!profile.text.contains("deny file-read*"));
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
