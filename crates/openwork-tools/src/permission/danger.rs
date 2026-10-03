//! 危险命令检测（permissions.md §10.1–§10.3）。
//!
//! 只回答"这条命令会不会批量丢弃未提交的工作、要不要先问一次"，不参与任何放行判断——
//! 边界是沙箱。尽力而为：程序名是动态的、或者脚本有语法错误时不检测，直接在沙箱里执行。
//! 判断只看程序名与标志，操作数里的 `$VAR`、glob 不影响结论。

use std::ops::Range;

use serde::{Deserialize, Serialize};
use tree_sitter::{Node, Parser, Tree};

/// 清单键（permissions.md §10.1）。清单是封闭的，加一条的标准只有一个：它在沙箱允许的
/// 范围内能批量丢弃未提交的工作。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DangerKey {
    /// `rm` 带 `-r` / `-R` / `-f` / `--recursive` / `--force`。
    RmRecursiveOrForce,
    /// `find` 带 `-delete`，或 `-exec` / `-execdir` 的命令是 `rm`。
    FindDelete,
    /// `git clean` 带 `-f` / `--force`。
    GitCleanForce,
    /// `bash -c` 套得太深，按命中处理（§4.4）。
    NestingTooDeep,
}

impl DangerKey {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::RmRecursiveOrForce => "rm_recursive_or_force",
            Self::FindDelete => "find_delete",
            Self::GitCleanForce => "git_clean_force",
            Self::NestingTooDeep => "nesting_too_deep",
        }
    }
}

/// 一次命中。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DangerMatch {
    pub key: DangerKey,
    /// 命中的那条命令在整条命令原文里的字节范围，卡片据此高亮；命中发生在
    /// `bash -c '…'` 内部时是外层那条 `bash -c` 命令。
    pub span: Range<usize>,
}

/// `bash -c` 的最大嵌套层数；更深按命中处理（与 Codex 一致）。
const MAX_NESTING: usize = 8;

/// 剥包装器的最多轮数；`sudo env nice …` 再长也不会超过它。
const MAX_WRAPPERS: usize = 16;

/// 找出第一条命中清单的命令。
pub fn detect(command: &str) -> Option<DangerMatch> {
    detect_in(command, 0).map(|(key, span)| DangerMatch { key, span })
}

fn detect_in(script: &str, depth: usize) -> Option<(DangerKey, Range<usize>)> {
    if depth > MAX_NESTING {
        return Some((DangerKey::NestingTooDeep, 0..script.len()));
    }
    let tree = parse(script)?;
    let root = tree.root_node();
    if root.has_error() {
        return None;
    }
    // 遇到不认识的节点也继续往下找 `command`（§4.5）：管道、子 shell、`$(...)`、
    // 控制流与函数体里的命令都要检查到。
    let mut pending = vec![root];
    while let Some(node) = pending.pop() {
        if node.kind() == "command"
            && let Some(key) = check_command(node, script, depth)
        {
            return Some((key, node.byte_range()));
        }
        let mut cursor = node.walk();
        let children = node.children(&mut cursor).collect::<Vec<_>>();
        pending.extend(children.into_iter().rev());
    }
    None
}

fn parse(script: &str) -> Option<Tree> {
    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_bash::LANGUAGE.into())
        .ok()?;
    parser.parse(script, None)
}

/// 命令里的一个词：字面量，或者含展开、只有运行时才知道的值。
#[derive(Debug, Clone, PartialEq, Eq)]
enum Word {
    Static(String),
    Dynamic,
}

impl Word {
    fn text(&self) -> Option<&str> {
        match self {
            Self::Static(text) => Some(text),
            Self::Dynamic => None,
        }
    }
}

fn check_command(node: Node<'_>, source: &str, depth: usize) -> Option<DangerKey> {
    let words = command_words(node, source)?;
    let words = strip_wrappers(&words)?;
    let (name, arguments) = words.split_first()?;
    match program(name.text()?) {
        "rm" => rm_is_recursive_or_forced(arguments).then_some(DangerKey::RmRecursiveOrForce),
        "find" => find_deletes(arguments).then_some(DangerKey::FindDelete),
        "git" => git_cleans_forcibly(arguments).then_some(DangerKey::GitCleanForce),
        "bash" | "sh" | "zsh" | "dash" | "ksh" => {
            let script = inline_script(arguments)?;
            detect_in(script, depth + 1).map(|(key, _)| key)
        }
        _ => None,
    }
}

/// 程序名与参数；程序名缺失时返回 `None`。变量赋值与重定向不算词。
fn command_words(node: Node<'_>, source: &str) -> Option<Vec<Word>> {
    let name = node.child_by_field_name("name")?;
    let mut words = vec![word(name, source)];
    let mut cursor = node.walk();
    words.extend(
        node.children_by_field_name("argument", &mut cursor)
            .map(|argument| word(argument, source)),
    );
    Some(words)
}

fn word(node: Node<'_>, source: &str) -> Word {
    static_text(node, source).map_or(Word::Dynamic, Word::Static)
}

/// 节点在不执行任何展开的情况下的字面值；含展开时返回 `None`。
fn static_text(node: Node<'_>, source: &str) -> Option<String> {
    let text = node.utf8_text(source.as_bytes()).ok()?;
    match node.kind() {
        "command_name" => {
            let mut cursor = node.walk();
            let children = node.named_children(&mut cursor).collect::<Vec<_>>();
            match children.as_slice() {
                [only] => static_text(*only, source),
                _ => None,
            }
        }
        "word" | "number" => Some(unescape(text)),
        "raw_string" => Some(text.trim_matches('\'').to_string()),
        "string" => {
            let mut cursor = node.walk();
            if node
                .named_children(&mut cursor)
                .all(|child| child.kind() == "string_content")
            {
                Some(unescape(text.trim_matches('"')))
            } else {
                None
            }
        }
        "concatenation" => {
            let mut cursor = node.walk();
            node.named_children(&mut cursor)
                .map(|part| static_text(part, source))
                .collect()
        }
        _ => None,
    }
}

/// 去掉反斜杠转义。只用来认出程序名和标志，所以不必区分引号内外的细微规则。
fn unescape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut characters = text.chars();
    while let Some(character) = characters.next() {
        if character == '\\' {
            if let Some(escaped) = characters.next() {
                out.push(escaped);
            }
        } else {
            out.push(character);
        }
    }
    out
}

fn program(name: &str) -> &str {
    name.rsplit('/').next().unwrap_or(name)
}

/// 剥掉包装器及其选项（§4.4）。`command -v` 只是查找程序、不执行，返回 `None`。
fn strip_wrappers(words: &[Word]) -> Option<&[Word]> {
    let mut rest = words;
    for _ in 0..MAX_WRAPPERS {
        let Some(name) = rest.first().and_then(Word::text) else {
            return Some(rest);
        };
        let after = &rest[1..];
        rest = match program(name) {
            "sudo" => skip_options(
                after,
                &["-u", "-g", "-C", "-D", "-h", "-p", "-U", "-r", "-t", "-T"],
            ),
            "env" => skip_assignments(skip_options(after, &["-u", "-C", "-S", "-P"])),
            "xargs" => skip_options(after, &["-I", "-L", "-n", "-P", "-s", "-d", "-E", "-a"]),
            "timeout" => skip_options(after, &["-s", "-k"]).get(1..).unwrap_or(&[]),
            "nice" => skip_options(after, &["-n"]),
            "nohup" => after,
            "time" => skip_options(after, &[]),
            "command" => {
                if after
                    .iter()
                    .map_while(|word| word.text().filter(|text| text.starts_with('-')))
                    .any(|option| option.contains('v') || option.contains('V'))
                {
                    return None;
                }
                skip_options(after, &[])
            }
            _ => return Some(rest),
        };
    }
    Some(rest)
}

/// 跳过开头的选项；`with_argument` 里的选项连同它后面的参数一起跳过。
fn skip_options<'a>(words: &'a [Word], with_argument: &[&str]) -> &'a [Word] {
    let mut index = 0;
    while let Some(text) = words.get(index).and_then(Word::text) {
        if text == "--" {
            return &words[index + 1..];
        }
        if !text.starts_with('-') || text == "-" {
            break;
        }
        index += if with_argument.contains(&text) { 2 } else { 1 };
    }
    words.get(index..).unwrap_or(&[])
}

fn skip_assignments(words: &[Word]) -> &[Word] {
    let count = words
        .iter()
        .take_while(|word| word.text().is_some_and(|text| text.contains('=')))
        .count();
    &words[count..]
}

/// 选项部分里的静态词；遇到 `--` 为止。
fn options(arguments: &[Word]) -> impl Iterator<Item = &str> {
    arguments
        .iter()
        .filter_map(Word::text)
        .take_while(|text| *text != "--")
        .filter(|text| text.starts_with('-') && *text != "-")
}

/// 单横线的短选项簇里是否含 `letter`：`-rf` 含 `r` 与 `f`。
fn short_cluster_has(option: &str, letters: &[char]) -> bool {
    !option.starts_with("--") && option[1..].chars().any(|flag| letters.contains(&flag))
}

fn rm_is_recursive_or_forced(arguments: &[Word]) -> bool {
    options(arguments).any(|option| {
        matches!(option, "--recursive" | "--force") || short_cluster_has(option, &['r', 'R', 'f'])
    })
}

fn find_deletes(arguments: &[Word]) -> bool {
    let words = arguments.iter().map(Word::text).collect::<Vec<_>>();
    words.iter().enumerate().any(|(index, word)| match word {
        Some("-delete") => true,
        Some("-exec" | "-execdir") => words
            .get(index + 1)
            .copied()
            .flatten()
            .is_some_and(|command| program(command) == "rm"),
        _ => false,
    })
}

fn git_cleans_forcibly(arguments: &[Word]) -> bool {
    let rest = skip_options(
        arguments,
        &[
            "-C",
            "-c",
            "--git-dir",
            "--work-tree",
            "--namespace",
            "--exec-path",
        ],
    );
    let Some((subcommand, rest)) = rest.split_first() else {
        return false;
    };
    subcommand.text() == Some("clean")
        && options(rest).any(|option| option == "--force" || short_cluster_has(option, &['f']))
}

/// `bash -c '<字面量>'` 的脚本；`-c` 可以和其他短选项合写（`-lc`）。
fn inline_script(arguments: &[Word]) -> Option<&str> {
    let position = arguments.iter().position(|word| {
        word.text()
            .is_some_and(|text| text.starts_with('-') && short_cluster_has(text, &['c']))
    })?;
    arguments.get(position + 1)?.text()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(command: &str) -> Option<DangerKey> {
        detect(command).map(|found| found.key)
    }

    /// permissions.md §15 #26–#29 列出的写法，逐条原样。
    #[test]
    fn acc_26_27_28_29_the_listed_commands_hit_or_miss_as_specified() {
        use DangerKey::{FindDelete, GitCleanForce, RmRecursiveOrForce};
        for (command, expected) in [
            ("rm -rf src", Some(RmRecursiveOrForce)),
            ("rm -f a.log", Some(RmRecursiveOrForce)),
            ("find . -name '*.o' -delete", Some(FindDelete)),
            ("find . -exec rm {} +", Some(FindDelete)),
            ("git clean -fdx", Some(GitCleanForce)),
            ("cargo build && rm -rf target", Some(RmRecursiveOrForce)),
            (
                r#"for d in a b; do rm -rf "$d"; done"#,
                Some(RmRecursiveOrForce),
            ),
            ("xargs rm -rf < list", Some(RmRecursiveOrForce)),
            ("sudo rm -rf x", Some(RmRecursiveOrForce)),
            ("env FOO=1 rm -rf x", Some(RmRecursiveOrForce)),
            ("bash -c 'rm -rf src'", Some(RmRecursiveOrForce)),
            ("rm -rf $DIR", Some(RmRecursiveOrForce)),
            ("$CMD -rf x", None),
            ("rm a.txt", None),
            ("echo rm -rf x", None),
            ("git reset --hard", None),
            ("git restore src/lib.rs", None),
            ("git checkout -- .", None),
        ] {
            assert_eq!(key(command), expected, "{command}");
        }
    }

    #[test]
    fn rm_needs_a_recursive_or_force_flag() {
        assert_eq!(key("rm -rf target"), Some(DangerKey::RmRecursiveOrForce));
        assert_eq!(key("rm -R build"), Some(DangerKey::RmRecursiveOrForce));
        assert_eq!(key("rm --force a"), Some(DangerKey::RmRecursiveOrForce));
        assert_eq!(key("/bin/rm -f a"), Some(DangerKey::RmRecursiveOrForce));
        assert_eq!(key("rm -rf $DIR"), Some(DangerKey::RmRecursiveOrForce));
        assert_eq!(key("rm a.txt"), None);
        assert_eq!(key("rm -- -rf"), None);
    }

    #[test]
    fn only_commands_in_name_position_count() {
        assert_eq!(key("echo rm -rf x"), None);
        assert_eq!(key("$CMD -rf x"), None);
        assert_eq!(key("for f in *.rs; do echo $f; done"), None);
    }

    #[test]
    fn every_subcommand_is_checked() {
        let command = "ls && (cd x && rm -r build) | cat";
        let found = detect(command).expect("hit");
        assert_eq!(&command[found.span], "rm -r build");
        assert_eq!(key("echo $(rm -rf x)"), Some(DangerKey::RmRecursiveOrForce));
        assert_eq!(key("FOO=1 rm -rf x"), Some(DangerKey::RmRecursiveOrForce));
    }

    #[test]
    fn wrappers_are_stripped() {
        for command in [
            "sudo rm -rf /",
            "sudo -u root rm -rf x",
            "env FOO=1 rm -f x",
            "find . | xargs rm -rf",
            "timeout 10 rm -r x",
            "nice -n 5 rm -rf x",
            "nohup rm -rf x",
            "time rm -rf x",
            "command rm -rf x",
        ] {
            assert_eq!(
                key(command),
                Some(DangerKey::RmRecursiveOrForce),
                "{command}"
            );
        }
        assert_eq!(key("command -v rm"), None);
    }

    #[test]
    fn find_deletes_directly_or_through_rm() {
        assert_eq!(
            key("find . -name '*.o' -delete"),
            Some(DangerKey::FindDelete)
        );
        assert_eq!(key("find . -exec rm {} \\;"), Some(DangerKey::FindDelete));
        assert_eq!(key("find . -name x -print"), None);
    }

    #[test]
    fn git_clean_needs_force() {
        assert_eq!(key("git clean -fd"), Some(DangerKey::GitCleanForce));
        assert_eq!(
            key("git -C sub clean --force"),
            Some(DangerKey::GitCleanForce)
        );
        assert_eq!(key("git clean -n"), None);
        assert_eq!(key("git status"), None);
    }

    #[test]
    fn inline_scripts_are_checked_and_highlight_the_outer_command() {
        let command = "sh -lc 'git clean -fdx'";
        let found = detect(command).expect("hit");
        assert_eq!(found.key, DangerKey::GitCleanForce);
        assert_eq!(found.span, 0..command.len());
        assert_eq!(
            key("bash -c \"rm -rf x\""),
            Some(DangerKey::RmRecursiveOrForce)
        );
        assert_eq!(key("bash script.sh"), None);
    }

    #[test]
    fn nesting_beyond_the_limit_counts_as_a_hit() {
        assert_eq!(
            detect_in("true", MAX_NESTING + 1).map(|(key, _)| key),
            Some(DangerKey::NestingTooDeep)
        );
    }

    #[test]
    fn syntax_errors_are_not_checked() {
        assert_eq!(key("rm -rf ("), None);
    }
}
