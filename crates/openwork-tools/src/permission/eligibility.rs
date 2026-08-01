use std::path::Path;

use crate::policy::{lexical_normalize, path_is_within};

pub(crate) struct EligibilityInput<'a> {
    pub(crate) program: &'a str,
    pub(crate) args: &'a [String],
    pub(crate) workspace: &'a Path,
    pub(crate) path: Option<&'a str>,
    pub(crate) has_leading_assignment: bool,
}

pub(crate) fn is_allow_eligible(input: EligibilityInput<'_>) -> bool {
    !input.has_leading_assignment
        && !is_interpreter_or_wrapper(input.program, input.args)
        && !has_escape_flag(input.program, input.args)
        && !resolves_from_workspace(input.program, input.workspace, input.path)
}

pub(crate) fn changes_working_directory(program: &str, args: &[String]) -> bool {
    let program = basename(program);
    matches!(program, "cd" | "pushd")
        || (program == "env"
            && args
                .iter()
                .any(|argument| argument == "-C" || argument.starts_with("--chdir=")))
}

fn is_interpreter_or_wrapper(program: &str, args: &[String]) -> bool {
    let program = basename(program);
    matches!(
        program,
        "sh" | "bash"
            | "zsh"
            | "dash"
            | "ksh"
            | "fish"
            | "csh"
            | "tcsh"
            | "ash"
            | "busybox"
            | "node"
            | "ruby"
            | "perl"
            | "awk"
            | "gawk"
            | "mawk"
            | "php"
            | "lua"
            | "tclsh"
            | "Rscript"
            | "osascript"
            | "deno"
            | "bun"
            | "ssh"
            | "xargs"
            | "env"
            | "timeout"
            | "nice"
            | "stdbuf"
            | "sudo"
            | "doas"
            | "nohup"
            | "setsid"
            | "script"
            | "watch"
            | "flock"
            | "chroot"
            | "ionice"
            | "taskset"
    ) || program.starts_with("python")
        || (program == "npm" && args.first().is_some_and(|argument| argument == "run"))
}

fn has_escape_flag(program: &str, args: &[String]) -> bool {
    let flags: &[&str] = match basename(program) {
        "git" => &[
            "-c",
            "-C",
            "--config-env",
            "--exec-path",
            "--git-dir",
            "--work-tree",
            "--ext-diff",
            "--textconv",
            "--paginate",
        ],
        "find" => &[
            "-exec", "-execdir", "-ok", "-okdir", "-delete", "-fprint", "-fprintf", "-fls",
        ],
        "rg" => &["--pre", "--pre-glob", "--hostname-bin"],
        "base64" => &["-o", "--output"],
        _ => return false,
    };

    args.iter()
        .any(|argument| flags.iter().any(|flag| flag_matches(flag, argument)))
}

fn flag_matches(flag: &str, argument: &str) -> bool {
    argument == flag
        || (flag.starts_with("--") && argument.starts_with(&format!("{flag}=")))
        || (flag.len() == 2 && argument.starts_with(flag) && argument.len() > flag.len())
}

fn resolves_from_workspace(program: &str, workspace: &Path, path: Option<&str>) -> bool {
    if program.contains('/') {
        let program = Path::new(program);
        let unresolved = if program.is_absolute() {
            program.to_path_buf()
        } else {
            workspace.join(program)
        };
        return path_is_within(&lexical_normalize(&unresolved), workspace);
    }

    let Some(path) = path else {
        return true;
    };
    std::env::split_paths(path).any(|entry| {
        entry.as_os_str().is_empty()
            || !entry.is_absolute()
            || path_is_within(&lexical_normalize(&entry), workspace)
    })
}

fn basename(program: &str) -> &str {
    Path::new(program)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(program)
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{EligibilityInput, changes_working_directory, is_allow_eligible};

    fn eligible(program: &str, args: &[&str], path: Option<&str>) -> bool {
        let args = args
            .iter()
            .map(|argument| (*argument).to_string())
            .collect::<Vec<_>>();
        is_allow_eligible(EligibilityInput {
            program,
            args: &args,
            workspace: Path::new("/repo"),
            path,
            has_leading_assignment: false,
        })
    }

    #[test]
    fn acc_30_33_and_34_wrappers_and_escape_flags_are_ineligible() {
        assert!(!eligible(
            "/bin/python3",
            &["-c", "print(1)"],
            Some("/usr/bin")
        ));
        assert!(!eligible("timeout", &["5", "ls"], Some("/usr/bin")));
        assert!(!eligible("npm", &["run", "build"], Some("/usr/bin")));
        assert!(!eligible(
            "git",
            &["-c", "core.pager=X", "log"],
            Some("/usr/bin")
        ));
        assert!(!eligible("rg", &["--pre=bash", "x"], Some("/usr/bin")));
    }

    #[test]
    fn acc_30_p3_interpreters_shells_privilege_tools_and_wrappers_are_ineligible() {
        for program in [
            "awk",
            "gawk",
            "mawk",
            "php",
            "lua",
            "tclsh",
            "Rscript",
            "osascript",
            "deno",
            "bun",
            "dash",
            "ksh",
            "fish",
            "csh",
            "tcsh",
            "ash",
            "busybox",
            "sudo",
            "doas",
            "nohup",
            "setsid",
            "script",
            "watch",
            "flock",
            "chroot",
            "ionice",
            "taskset",
        ] {
            assert!(
                !eligible(program, &[], Some("/usr/bin")),
                "{program} must not be eligible for automatic exec allow"
            );
        }
    }

    #[test]
    fn acc_36_path_resolution_is_strictly_lexical() {
        assert!(!eligible("./ls", &[], Some("/usr/bin")));
        assert!(!eligible("ls", &[], Some("/usr/bin:/repo/bin")));
        assert!(!eligible("ls", &[], Some("/usr/bin:bin")));
        assert!(!eligible("ls", &[], Some("/usr/bin:")));
        assert!(eligible("/bin/ls", &[], Some("/repo/bin")));
        assert!(eligible("ls", &[], Some("/usr/bin:/bin")));
    }

    #[test]
    fn acc_35_cwd_changes_are_detected_for_the_whole_script_gate() {
        assert!(changes_working_directory("cd", &["/tmp".to_string()]));
        assert!(changes_working_directory("pushd", &["/tmp".to_string()]));
        assert!(changes_working_directory(
            "env",
            &["-C".to_string(), "/tmp".to_string(), "ls".to_string()]
        ));
    }
}
