//! 路径的四档（permissions.md §2.3）——只有这里知道它们。
//!
//! 每条规则有两种表达，必须一致：给进程内文件工具围栏用的、作用于规范化路径的匹配函数，
//! 以及给内核用的 Seatbelt 过滤器。`tests/parity.rs` 的对等测试把两者绑在一起。
//!
//! 工作区内的名字按 ASCII 大小写不敏感匹配。macOS 卷默认大小写不敏感：规范化会把已存在的
//! 路径段还原成磁盘上的写法，但新建的 `.ENV` 保留调用方的拼写，dotenv 仍会把它当 `.env` 读。

use std::path::{Component, Path, PathBuf};

/// 在工作区内任意位置出现时，整个子树都属于敏感档的目录。
pub(crate) const SENSITIVE_DIRECTORIES: &[&str] = &[".git", ".vscode", ".idea"];

/// 在工作区内属于敏感档的文件名（路径的最后一段）。
pub(crate) const SENSITIVE_FILES: &[&str] = &[
    ".gitconfig",
    ".gitmodules",
    ".bashrc",
    ".bash_profile",
    ".zshrc",
    ".zprofile",
    ".profile",
];

/// 最后一段以它开头的都属于敏感档：`.env`、`.env.local`、`.envrc`。
pub(crate) const SENSITIVE_FILE_PREFIX: &str = ".env";

/// `$HOME` 下的凭据位置：除非越界授权点名，否则不可读。
pub(crate) const CREDENTIAL_PATHS: &[&str] = &[
    ".ssh",
    ".aws",
    ".gnupg",
    ".kube",
    ".azure",
    ".config/gh",
    ".config/gcloud",
    ".docker/config.json",
    ".netrc",
    ".git-credentials",
    ".npmrc",
    ".pypirc",
    ".cargo/credentials.toml",
    "Library/Keychains",
    "Library/Cookies",
    "Library/Safari",
    "Library/Application Support/Google/Chrome",
    "Library/Application Support/Firefox",
];

/// 所有命令都可写的字符设备（permissions.md §3.1）。在 `/dev` 下创建文件仍然不可能。
///
/// 不列 `/dev/stdout`、`/dev/stderr`：它们是指向 `/dev/fd/1`、`/dev/fd/2` 的符号链接，
/// Seatbelt 按解析后的路径判断，文件工具围栏拿到的也是规范化后的路径，
/// 所以由下面的 `/dev/fd/<n>` 覆盖。
pub(crate) const WRITABLE_DEVICES: &[&str] = &["/dev/null", "/dev/zero", "/dev/tty"];

/// `/dev/fd/<n>` 与 `/dev/ttys<n>`。
pub(crate) fn is_writable_device(path: &Path) -> bool {
    if WRITABLE_DEVICES
        .iter()
        .any(|device| path == Path::new(device))
    {
        return true;
    }
    let Some(text) = path.to_str() else {
        return false;
    };
    ["/dev/fd/", "/dev/ttys"].iter().any(|prefix| {
        text.strip_prefix(prefix)
            .is_some_and(|rest| !rest.is_empty() && rest.bytes().all(|byte| byte.is_ascii_digit()))
    })
}

/// [`is_writable_device`] 中带编号设备对应的 Seatbelt 正则。
pub(crate) const WRITABLE_DEVICE_REGEXES: &[&str] = &["^/dev/fd/[0-9]+$", "^/dev/ttys[0-9]+$"];

/// `path` 在 `root` 之下时返回其下的各段；不在其下时返回 `None`。
fn components_below<'a>(root: &Path, path: &'a Path) -> Option<Vec<&'a str>> {
    let relative = path.strip_prefix(root).ok()?;
    relative
        .components()
        .map(|component| match component {
            Component::Normal(name) => name.to_str(),
            _ => None,
        })
        .collect()
}

/// 工作区内任意位置的 `.git/hooks/**`：那里的脚本会在用户下一次 `git commit` 时在沙箱外执行。
pub(crate) fn is_workspace_hard_protected(workspace: &Path, path: &Path) -> bool {
    components_below(workspace, path).is_some_and(|components| {
        components.windows(2).any(|pair| {
            pair[0].eq_ignore_ascii_case(".git") && pair[1].eq_ignore_ascii_case("hooks")
        })
    })
}

pub(crate) fn is_workspace_sensitive(workspace: &Path, path: &Path) -> bool {
    let Some(components) = components_below(workspace, path) else {
        return false;
    };
    let named = |names: &[&str], component: &str| {
        names
            .iter()
            .any(|name| name.eq_ignore_ascii_case(component))
    };
    if components
        .iter()
        .any(|component| named(SENSITIVE_DIRECTORIES, component))
    {
        return true;
    }
    components.last().is_some_and(|last| {
        named(SENSITIVE_FILES, last)
            || last
                .as_bytes()
                .get(..SENSITIVE_FILE_PREFIX.len())
                .is_some_and(|start| start.eq_ignore_ascii_case(SENSITIVE_FILE_PREFIX.as_bytes()))
    })
}

/// 以工作区为锚点的 Seatbelt 正则，由同一组清单生成。
pub(crate) fn workspace_hard_protected_regex(workspace: &Path) -> String {
    format!(
        r"^{}/(.*/)?{}/{}(/.*)?$",
        escape_regex(workspace),
        case_insensitive(".git"),
        case_insensitive("hooks")
    )
}

pub(crate) fn workspace_sensitive_regexes(workspace: &Path) -> Vec<String> {
    let root = escape_regex(workspace);
    let alternatives = |names: &[&str]| {
        names
            .iter()
            .map(|name| case_insensitive(name))
            .collect::<Vec<_>>()
            .join("|")
    };
    vec![
        format!(
            r"^{root}/(.*/)?({})(/.*)?$",
            alternatives(SENSITIVE_DIRECTORIES)
        ),
        format!(r"^{root}/(.*/)?({})$", alternatives(SENSITIVE_FILES)),
        format!(
            r"^{root}/(.*/)?{}[^/]*$",
            case_insensitive(SENSITIVE_FILE_PREFIX)
        ),
    ]
}

pub(crate) fn credential_paths(home: &Path) -> Vec<PathBuf> {
    CREDENTIAL_PATHS
        .iter()
        .map(|path| home.join(path))
        .collect()
}

/// 转义路径以嵌入 Seatbelt 正则。路径以 `-D` 参数进入 profile，从不出现在正文里，
/// 所以转义只需让每个字符对正则引擎而言是字面量。
pub(crate) fn escape_regex(path: &Path) -> String {
    escape_regex_str(&path.to_string_lossy())
}

/// 以任意 ASCII 大小写匹配 `name` 的正则：`.env` → `\.[eE][nN][vV]`。
/// Seatbelt 的正则没有大小写不敏感的开关。
fn case_insensitive(name: &str) -> String {
    name.chars()
        .map(|character| {
            if character.is_ascii_alphabetic() {
                format!(
                    "[{}{}]",
                    character.to_ascii_lowercase(),
                    character.to_ascii_uppercase()
                )
            } else {
                escape_regex_str(&character.to_string())
            }
        })
        .collect()
}

fn escape_regex_str(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    for character in text.chars() {
        if r"\.^$|?*+()[]{}".contains(character) {
            escaped.push('\\');
        }
        escaped.push(character);
    }
    escaped
}

#[cfg(test)]
mod tests {
    use super::*;

    const WS: &str = "/work/project";

    fn sensitive(path: &str) -> bool {
        is_workspace_sensitive(Path::new(WS), Path::new(path))
    }

    #[test]
    fn sensitive_directories_cover_their_subtree_at_any_depth() {
        assert!(sensitive("/work/project/.git"));
        assert!(sensitive("/work/project/.git/index.lock"));
        assert!(sensitive("/work/project/vendor/lib/.git/config"));
        assert!(sensitive("/work/project/.vscode/settings.json"));
        assert!(!sensitive("/work/project/src/.gitignore"));
        assert!(!sensitive("/work/project/foo.git"));
        assert!(!sensitive("/work/project/.github/workflows/ci.yml"));
    }

    #[test]
    fn sensitive_files_match_the_final_component_only() {
        assert!(sensitive("/work/project/.env"));
        assert!(sensitive("/work/project/app/.env.local"));
        assert!(sensitive("/work/project/.envrc"));
        assert!(sensitive("/work/project/.zshrc"));
        assert!(!sensitive("/work/project/.envdir/inner"));
        assert!(sensitive("/work/project/.ENV"));
        assert!(sensitive("/work/project/.Env.local"));
        assert!(sensitive("/work/project/.GIT/config"));
        assert!(sensitive("/work/project/.VSCode/settings.json"));
        assert!(sensitive("/work/project/.ZSHRC"));
        assert!(!sensitive("/work/project/.zshrc.bak"));
        assert!(!sensitive("/elsewhere/.env"));
    }

    #[test]
    fn hooks_are_hard_protected_at_any_depth() {
        let ws = Path::new(WS);
        assert!(is_workspace_hard_protected(
            ws,
            Path::new("/work/project/.git/hooks")
        ));
        assert!(is_workspace_hard_protected(
            ws,
            Path::new("/work/project/sub/.git/hooks/pre-commit")
        ));
        assert!(!is_workspace_hard_protected(
            ws,
            Path::new("/work/project/.git/config")
        ));
        assert!(!is_workspace_hard_protected(
            ws,
            Path::new("/work/project/hooks")
        ));
        assert!(is_workspace_hard_protected(
            ws,
            Path::new("/work/project/.GIT/Hooks/pre-commit")
        ));
    }

    #[test]
    fn case_insensitive_names_become_character_classes() {
        assert_eq!(case_insensitive(".git"), r"\.[gG][iI][tT]");
        assert_eq!(
            case_insensitive(".bash_profile"),
            r"\.[bB][aA][sS][hH]_[pP][rR][oO][fF][iI][lL][eE]"
        );
    }

    #[test]
    fn regex_escaping_makes_metacharacters_literal() {
        assert_eq!(
            escape_regex(Path::new(r#"/a b/"q"(x)[y]{z}.*+?^$|\"#)),
            r#"/a b/"q"\(x\)\[y\]\{z\}\.\*\+\?\^\$\|\\"#
        );
    }

    #[test]
    fn numbered_devices_are_writable_but_other_dev_paths_are_not() {
        assert!(is_writable_device(Path::new("/dev/null")));
        assert!(is_writable_device(Path::new("/dev/fd/1")));
        assert!(is_writable_device(Path::new("/dev/fd/3")));
        // 调用方传入的是规范化后的路径，`/dev/stdout` 已解析成 `/dev/fd/1`。
        assert!(!is_writable_device(Path::new("/dev/stdout")));
        assert!(is_writable_device(Path::new("/dev/ttys004")));
        assert!(!is_writable_device(Path::new("/dev/fd/")));
        assert!(!is_writable_device(Path::new("/dev/disk0")));
        assert!(!is_writable_device(Path::new("/dev/fd/3/x")));
    }
}
