mod table;

use std::path::{Path, PathBuf};

use crate::policy::lexical_normalize;

use super::{Effect, ReadonlyProof};
use table::{Arity, COMMANDS, OperandKind, ReadonlyCommand};

pub(crate) struct Proof {
    pub(crate) marker: ReadonlyProof,
    pub(crate) effects: Vec<Effect>,
}

pub(crate) fn prove(program: &str, args: &[String], workspace: &Path) -> Option<Proof> {
    let basename = Path::new(program)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(program);
    if basename == "sed" {
        return prove_sed(args, workspace);
    }
    if std::iter::once(program)
        .chain(args.iter().map(String::as_str))
        .any(dynamic_token)
    {
        return None;
    }

    let (command, command_args) = match_command(program, args)?;
    let operands = parse_operands(command, command_args)?;
    if command.operands == OperandKind::PatternThenPaths
        && operands
            .first()
            .is_some_and(|pattern| pattern.contains(['\n', '\r']))
    {
        return None;
    }

    let path_operands = match command.operands {
        OperandKind::AllPaths => operands.as_slice(),
        OperandKind::PatternThenPaths => operands.get(1..).unwrap_or_default(),
        OperandKind::NoPaths => &[],
        OperandKind::None if operands.is_empty() => &[],
        OperandKind::None => return None,
    };
    let mut effects = Vec::with_capacity(path_operands.len() + 1);
    effects.push(Effect::read(workspace));
    effects.extend(
        path_operands
            .iter()
            .filter(|operand| operand.as_str() != "-")
            .map(|operand| Effect::read(resolve_effect_path(workspace, operand))),
    );

    Some(Proof {
        marker: ReadonlyProof {
            key: command.key.to_string(),
        },
        effects,
    })
}

fn prove_sed(args: &[String], workspace: &Path) -> Option<Proof> {
    let invocation = super::sed::analyze(args)?;
    if invocation.mode != super::sed::Mode::Readonly {
        return None;
    }
    let mut effects = Vec::with_capacity(invocation.files.len() + 1);
    effects.push(Effect::read(workspace));
    effects.extend(
        invocation
            .files
            .iter()
            .filter(|path| path.as_str() != "-")
            .map(|path| Effect::read(resolve_effect_path(workspace, path))),
    );
    Some(Proof {
        marker: ReadonlyProof {
            key: "sed".to_string(),
        },
        effects,
    })
}

fn match_command<'a>(
    program: &str,
    args: &'a [String],
) -> Option<(&'static ReadonlyCommand, &'a [String])> {
    let basename = Path::new(program)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(program);
    COMMANDS.iter().find_map(|command| {
        let mut key = command.key.split_ascii_whitespace();
        if key.next()? != basename {
            return None;
        }
        let suffix = key.collect::<Vec<_>>();
        if suffix.len() > args.len()
            || !suffix
                .iter()
                .zip(args.iter())
                .all(|(expected, actual)| expected == actual)
        {
            return None;
        }
        Some((command, &args[suffix.len()..]))
    })
}

fn parse_operands<'a>(command: &ReadonlyCommand, args: &'a [String]) -> Option<Vec<&'a String>> {
    let mut operands = Vec::new();
    let mut index = 0;
    let mut options_ended = false;
    while index < args.len() {
        let argument = &args[index];
        if !options_ended && argument == "--" {
            options_ended = true;
            index += 1;
            continue;
        }
        if !options_ended && argument.starts_with('-') && argument != "-" {
            let exact_arity = command
                .flags
                .iter()
                .find_map(|(registered, arity)| (*registered == argument).then_some(*arity));
            let (flag, attached_value) = split_long_flag(argument);
            let arity = exact_arity.or_else(|| {
                command
                    .flags
                    .iter()
                    .find_map(|(registered, arity)| (*registered == flag).then_some(*arity))
            });
            let Some(arity) = arity else {
                // `-la` is `-l -a`. Accept a cluster only when every letter in
                // it is a registered no-argument flag: one that takes a value
                // would consume the next token, and mis-reading that is the
                // parser differential permissions.md §2.3② warns about.
                if is_short_cluster(argument) && cluster_is_all_nullary(command, argument) {
                    index += 1;
                    continue;
                }
                return None;
            };
            let attached_value = exact_arity.is_none().then_some(attached_value).flatten();
            match (arity, attached_value) {
                (Arity::None, Some(_)) => return None,
                (Arity::None, None) | (Arity::One, Some(_)) => index += 1,
                (Arity::One, None) => {
                    if index + 1 >= args.len() {
                        return None;
                    }
                    index += 2;
                }
            }
            continue;
        }
        operands.push(argument);
        index += 1;
    }
    if command
        .max_operands
        .is_some_and(|maximum| operands.len() > maximum)
    {
        return None;
    }
    Some(operands)
}

/// A bundle of short flags such as `-la` — one dash, two or more characters.
fn is_short_cluster(argument: &str) -> bool {
    !argument.starts_with("--") && argument.chars().skip(1).take(2).count() == 2
}

fn cluster_is_all_nullary(command: &ReadonlyCommand, argument: &str) -> bool {
    argument.chars().skip(1).all(|letter| {
        let single = format!("-{letter}");
        command
            .flags
            .iter()
            .any(|(registered, arity)| *registered == single && *arity == Arity::None)
    })
}

fn split_long_flag(argument: &str) -> (&str, Option<&str>) {
    if argument.starts_with("--")
        && let Some((flag, value)) = argument.split_once('=')
    {
        return (flag, Some(value));
    }
    (argument, None)
}

/// Tokens whose runtime value cannot be determined by reading them.
///
/// A leading `~` belongs here for the same reason `$` does: it is a *prefix
/// rewrite*, not an expansion bounded by the text around it. Treating
/// `~/.ssh/id_rsa` literally would resolve it to `<workspace>/~/.ssh/id_rsa`,
/// land inside the workspace, and auto-allow a read that bash performs against
/// `$HOME` — the exact parser differential permissions.md §2.3④ exists to
/// prevent. Glob metacharacters are deliberately *not* here: `*` and `?` never
/// cross a path separator, so the lexical prefix still bounds where they can
/// expand to.
fn dynamic_token(token: &str) -> bool {
    token.starts_with('~')
        || token.contains('$')
        || token.contains('`')
        || (token.contains('{') && (token.contains(',') || token.contains("..")))
}

fn resolve_effect_path(workspace: &Path, input: &str) -> PathBuf {
    let path = Path::new(input);
    let unresolved = if path.is_absolute() {
        path.to_path_buf()
    } else {
        workspace.join(path)
    };
    lexical_normalize(&unresolved)
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::prove;

    fn proof(command: &str, args: &[&str]) -> Option<String> {
        prove(
            command,
            &args
                .iter()
                .map(|argument| (*argument).to_string())
                .collect::<Vec<_>>(),
            Path::new("/repo"),
        )
        .map(|proof| proof.marker.key)
    }

    #[test]
    fn table_matches_multi_token_keys_before_program_keys() {
        assert_eq!(proof("git", &["status"]), Some("git status".to_string()));
        assert_eq!(proof("git", &["diff"]), Some("git diff".to_string()));
        assert_eq!(proof("git", &["commit"]), None);
    }

    /// permissions.md §2.3④: a leading `~` is a prefix rewrite, so the token
    /// cannot be resolved by reading it. Without this, `cat ~/.ssh/id_rsa`
    /// resolves to `<workspace>/~/.ssh/id_rsa`, lands inside the workspace,
    /// and auto-allows a read that bash performs against `$HOME`.
    #[test]
    fn tilde_prefixed_operands_are_never_provable() {
        assert_eq!(proof("cat", &["~/.ssh/id_rsa"]), None);
        assert_eq!(proof("cat", &["--", "~/.aws/credentials"]), None);
        assert_eq!(proof("ls", &["~root"]), None);
        assert_eq!(proof("ls", &["~"]), None);
        // `~` only expands at the start of a word — a literal one elsewhere is
        // an ordinary filename and stays provable.
        assert_eq!(proof("cat", &["src/back~up.rs"]), Some("cat".to_string()));
    }

    #[test]
    fn short_flag_clusters_are_provable_only_when_every_letter_is_nullary() {
        assert_eq!(proof("ls", &["-la", "src"]), Some("ls".to_string()));
        assert_eq!(proof("ls", &["-lAh", "src"]), Some("ls".to_string()));
        assert_eq!(proof("cat", &["-nE", "src/a.rs"]), Some("cat".to_string()));
        // `-z` is not registered at all.
        assert_eq!(proof("ls", &["-laz", "src"]), None);
        // `-S` takes a value, so it must never be swallowed inside a cluster.
        assert_eq!(proof("git", &["diff", "-Sx"]), None);
    }

    #[test]
    fn flag_arity_matches_the_program_parser() {
        assert_eq!(
            proof("git", &["diff", "-S", "needle"]),
            Some("git diff".to_string())
        );
        assert_eq!(proof("git", &["diff", "-S", "--", "--output=/tmp/x"]), None);
        assert_eq!(
            proof("ls", &["--color=always", "src"]),
            Some("ls".to_string())
        );
    }
}
