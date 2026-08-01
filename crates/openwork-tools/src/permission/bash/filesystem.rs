use std::path::{Path, PathBuf};

use crate::policy::lexical_normalize;

use super::super::Effect;
use super::super::sed;

pub(crate) struct Proof {
    pub(crate) effects: Vec<Effect>,
}

#[derive(Clone, Copy)]
enum FlagValue {
    None,
    Ignore,
    ReadPath,
}

#[derive(Clone, Copy)]
struct Flag {
    name: &'static str,
    value: FlagValue,
}

struct ParsedArgs<'a> {
    operands: Vec<&'a str>,
    reads: Vec<&'a str>,
}

pub(crate) fn prove(program: &str, args: &[String], workspace: &Path) -> Option<Proof> {
    let basename = Path::new(program)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(program);
    if basename != "sed" && args.iter().any(|argument| dynamic_token(argument)) {
        return None;
    }
    let effects = match basename {
        "mkdir" => writes(
            parse_args(
                args,
                &[
                    Flag {
                        name: "-p",
                        value: FlagValue::None,
                    },
                    Flag {
                        name: "-m",
                        value: FlagValue::Ignore,
                    },
                ],
            )?,
            workspace,
        )?,
        "rmdir" => writes(
            parse_args(
                args,
                &[Flag {
                    name: "-p",
                    value: FlagValue::None,
                }],
            )?,
            workspace,
        )?,
        "touch" => {
            let parsed = parse_args(
                args,
                &[
                    Flag {
                        name: "-a",
                        value: FlagValue::None,
                    },
                    Flag {
                        name: "-m",
                        value: FlagValue::None,
                    },
                    Flag {
                        name: "-c",
                        value: FlagValue::None,
                    },
                    Flag {
                        name: "-r",
                        value: FlagValue::ReadPath,
                    },
                ],
            )?;
            let mut effects = parsed
                .reads
                .iter()
                .map(|path| Effect::read(resolve_effect_path(workspace, path)))
                .collect::<Vec<_>>();
            effects.extend(writes(parsed, workspace)?);
            effects
        }
        "rm" => writes(
            parse_args(
                args,
                &[
                    Flag {
                        name: "-r",
                        value: FlagValue::None,
                    },
                    Flag {
                        name: "-R",
                        value: FlagValue::None,
                    },
                    Flag {
                        name: "-f",
                        value: FlagValue::None,
                    },
                ],
            )?,
            workspace,
        )?,
        "mv" => {
            let parsed = parse_args(args, &[])?;
            if parsed.operands.len() < 2 {
                return None;
            }
            parsed
                .operands
                .iter()
                .map(|path| write_effect(workspace, path))
                .collect::<Option<Vec<_>>>()?
        }
        "cp" => {
            let parsed = parse_args(
                args,
                &[
                    Flag {
                        name: "-r",
                        value: FlagValue::None,
                    },
                    Flag {
                        name: "-R",
                        value: FlagValue::None,
                    },
                ],
            )?;
            if parsed.operands.len() < 2 {
                return None;
            }
            let (target, sources) = parsed.operands.split_last()?;
            let mut effects = sources
                .iter()
                .map(|path| Effect::read(resolve_effect_path(workspace, path)))
                .collect::<Vec<_>>();
            effects.push(write_effect(workspace, target)?);
            effects
        }
        "sed" => {
            let invocation = sed::analyze(args)?;
            let sed::Mode::InPlace { backup_suffix } = invocation.mode else {
                return None;
            };
            let mut effects = invocation
                .files
                .iter()
                .map(|path| write_effect(workspace, path))
                .collect::<Option<Vec<_>>>()?;
            if let Some(suffix) = backup_suffix {
                for path in &invocation.files {
                    effects.push(write_effect(workspace, &format!("{path}{suffix}"))?);
                }
            }
            effects
        }
        _ => return None,
    };

    Some(Proof { effects })
}

fn parse_args<'a>(args: &'a [String], flags: &[Flag]) -> Option<ParsedArgs<'a>> {
    let mut operands = Vec::new();
    let mut reads = Vec::new();
    let mut index = 0;
    let mut options_ended = false;

    while index < args.len() {
        let argument = args[index].as_str();
        if !options_ended && argument == "--" {
            options_ended = true;
            index += 1;
            continue;
        }
        if !options_ended && argument.starts_with('-') && argument != "-" {
            if short_cluster_is_nullary(argument, flags) {
                index += 1;
                continue;
            }
            let flag = flags.iter().find(|flag| flag.name == argument)?;
            match flag.value {
                FlagValue::None => index += 1,
                FlagValue::Ignore | FlagValue::ReadPath => {
                    let value = args.get(index + 1)?.as_str();
                    if matches!(flag.value, FlagValue::ReadPath) {
                        reads.push(value);
                    }
                    index += 2;
                }
            }
            continue;
        }
        operands.push(argument);
        index += 1;
    }

    Some(ParsedArgs { operands, reads })
}

fn short_cluster_is_nullary(argument: &str, flags: &[Flag]) -> bool {
    !argument.starts_with("--")
        && argument.chars().skip(1).take(2).count() == 2
        && argument.chars().skip(1).all(|letter| {
            let name = format!("-{letter}");
            flags
                .iter()
                .any(|flag| flag.name == name && matches!(flag.value, FlagValue::None))
        })
}

fn writes(parsed: ParsedArgs<'_>, workspace: &Path) -> Option<Vec<Effect>> {
    (!parsed.operands.is_empty())
        .then(|| {
            parsed
                .operands
                .iter()
                .map(|path| write_effect(workspace, path))
                .collect::<Option<Vec<_>>>()
        })
        .flatten()
}

fn dynamic_token(token: &str) -> bool {
    token.starts_with('~') || token.contains(['$', '`'])
}

/// Resolves a token that will be *written to*, refusing glob patterns.
///
/// permissions.md §2.3④ draws the line at "does this notation rewrite the path
/// prefix", and glob stays provable for `read` because `*` never crosses a path
/// separator. That argument does not carry over to writes: the prefix bounds
/// *where* the expansion lands, not *how much* it destroys. Without this,
/// `rm -rf .` asks (§3.4) while `rm -rf *` — one character apart, and reaching
/// every non-dotfile in the workspace — runs silently. A protection that a
/// one-character change defeats reads as a guarantee and is not one.
pub(super) fn resolve_write_path(workspace: &Path, input: &str) -> Option<PathBuf> {
    if input.contains(['*', '?', '[']) {
        return None;
    }
    Some(resolve_effect_path(workspace, input))
}

fn write_effect(workspace: &Path, input: &str) -> Option<Effect> {
    resolve_write_path(workspace, input).map(Effect::write)
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
