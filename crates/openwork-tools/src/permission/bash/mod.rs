use std::path::{Path, PathBuf};

use tree_sitter::{Node, Parser};

use crate::policy::lexical_normalize;

use super::super::{AnalysisUnit, Effect, InvocationAnalysis};

pub(crate) fn analyze(raw: &str, workspace: &Path) -> InvocationAnalysis {
    let mut parser = Parser::new();
    if parser
        .set_language(&tree_sitter_bash::LANGUAGE.into())
        .is_err()
    {
        return InvocationAnalysis::unparsed(raw);
    }
    let Some(tree) = parser.parse(raw, None) else {
        return InvocationAnalysis::unparsed(raw);
    };
    let root = tree.root_node();
    if root.has_error() {
        return InvocationAnalysis::unparsed(raw);
    }

    let mut units = Vec::new();
    if collect_units(root, raw.as_bytes(), workspace, &mut units).is_err() || units.is_empty() {
        return InvocationAnalysis::unparsed(raw);
    }
    InvocationAnalysis::new(raw, units)
}

fn collect_units(
    node: Node<'_>,
    source: &[u8],
    workspace: &Path,
    units: &mut Vec<AnalysisUnit>,
) -> Result<(), ()> {
    match node.kind() {
        "program" | "list" | "pipeline" | "subshell" => {
            let mut cursor = node.walk();
            for child in node.named_children(&mut cursor) {
                collect_units(child, source, workspace, units)?;
            }
            Ok(())
        }
        "command" => {
            units.push(analyze_command(node, source, workspace)?);
            Ok(())
        }
        "redirected_statement" => {
            let body = node.child_by_field_name("body").ok_or(())?;
            let before = units.len();
            collect_units(body, source, workspace, units)?;
            if units.len() != before + 1 {
                return Err(());
            }
            let (effects, heredoc_units) = analyze_redirects(node, source, workspace)?;
            units[before].effects.extend(effects);
            units[before].display = node_text(node, source)?.to_string();
            units.extend(heredoc_units);
            Ok(())
        }
        "comment" => Ok(()),
        _ => Err(()),
    }
}

fn analyze_command(node: Node<'_>, source: &[u8], workspace: &Path) -> Result<AnalysisUnit, ()> {
    let name = node.child_by_field_name("name").ok_or(())?;
    let program = static_token(name, source)?;
    if program.is_empty() {
        return Err(());
    }

    let mut cursor = node.walk();
    let args = node
        .children_by_field_name("argument", &mut cursor)
        .map(|argument| static_token(argument, source))
        .collect::<Result<Vec<_>, _>>()?;

    let mut effects = vec![Effect::Exec {
        program: program.clone(),
        args: args.clone(),
    }];
    if Path::new(&program)
        .file_name()
        .is_some_and(|name| name == "cat")
    {
        effects.extend(cat_read_effects(&args, workspace));
    }
    let (redirection_effects, heredoc_units) = analyze_redirects(node, source, workspace)?;
    if !heredoc_units.is_empty() {
        return Err(());
    }
    effects.extend(redirection_effects);

    Ok(AnalysisUnit::new(node_text(node, source)?, effects))
}

fn cat_read_effects(args: &[String], workspace: &Path) -> Vec<Effect> {
    let mut operands = false;
    args.iter()
        .filter_map(|argument| {
            if argument == "--" {
                operands = true;
                return None;
            }
            if !operands && argument.starts_with('-') {
                return None;
            }
            if argument == "-" {
                return None;
            }
            Some(Effect::read(resolve_effect_path(workspace, argument)))
        })
        .collect()
}

fn analyze_redirects(
    node: Node<'_>,
    source: &[u8],
    workspace: &Path,
) -> Result<(Vec<Effect>, Vec<AnalysisUnit>), ()> {
    let mut effects = Vec::new();
    let mut heredoc_units = Vec::new();
    let mut cursor = node.walk();
    for redirect in node.children_by_field_name("redirect", &mut cursor) {
        match redirect.kind() {
            "file_redirect" => {
                effects.extend(file_redirect_effects(redirect, source, workspace)?);
            }
            "heredoc_redirect" => {
                collect_heredoc_units(redirect, source, workspace, &mut heredoc_units)?;
            }
            _ => return Err(()),
        }
    }
    Ok((effects, heredoc_units))
}

fn file_redirect_effects(
    redirect: Node<'_>,
    source: &[u8],
    workspace: &Path,
) -> Result<Vec<Effect>, ()> {
    let raw = node_text(redirect, source)?;
    if raw.contains(">&") || raw.contains("<&") {
        return Ok(Vec::new());
    }
    let destination = redirect.child_by_field_name("destination").ok_or(())?;
    let destination = static_token(destination, source)?;
    if destination == "/dev/null" {
        return Ok(Vec::new());
    }
    let path = resolve_effect_path(workspace, &destination);
    if raw.contains("<>") {
        Ok(vec![Effect::read(path.clone()), Effect::write(path)])
    } else if raw.contains('>') {
        Ok(vec![Effect::write(path)])
    } else if raw.contains('<') {
        Ok(vec![Effect::read(path)])
    } else {
        Err(())
    }
}

fn collect_heredoc_units(
    redirect: Node<'_>,
    source: &[u8],
    workspace: &Path,
    units: &mut Vec<AnalysisUnit>,
) -> Result<(), ()> {
    let mut cursor = redirect.walk();
    for child in redirect.named_children(&mut cursor) {
        match child.kind() {
            "heredoc_start" | "heredoc_end" => {}
            "heredoc_body" => {
                let mut body_cursor = child.walk();
                for body_child in child.named_children(&mut body_cursor) {
                    match body_child.kind() {
                        "heredoc_content" => {}
                        "command_substitution" => {
                            let command = body_child.named_child(0).ok_or(())?;
                            collect_units(command, source, workspace, units)?;
                        }
                        _ => return Err(()),
                    }
                }
            }
            _ => return Err(()),
        }
    }
    Ok(())
}

fn static_token(node: Node<'_>, source: &[u8]) -> Result<String, ()> {
    match node.kind() {
        "command_name" => {
            let mut cursor = node.walk();
            let mut children = node.named_children(&mut cursor);
            let child = children.next().ok_or(())?;
            if children.next().is_some() {
                return Err(());
            }
            static_token(child, source)
        }
        "word" => decode_word(node_text(node, source)?),
        "raw_string" => strip_quotes(node_text(node, source)?, '\''),
        "string" => {
            if node.named_child_count() != 0 {
                return Err(());
            }
            strip_quotes(node_text(node, source)?, '"')
        }
        _ => Err(()),
    }
}

fn strip_quotes(raw: &str, quote: char) -> Result<String, ()> {
    raw.strip_prefix(quote)
        .and_then(|value| value.strip_suffix(quote))
        .map(ToOwned::to_owned)
        .ok_or(())
}

fn decode_word(raw: &str) -> Result<String, ()> {
    if raw.contains(['$', '`', '{', '}']) {
        return Err(());
    }
    let mut decoded = String::new();
    let mut chars = raw.chars();
    while let Some(character) = chars.next() {
        if character == '\\' {
            decoded.push(chars.next().ok_or(())?);
        } else {
            decoded.push(character);
        }
    }
    Ok(decoded)
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

fn node_text<'a>(node: Node<'_>, source: &'a [u8]) -> Result<&'a str, ()> {
    node.utf8_text(source).map_err(|_| ())
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::analyze;
    use crate::Effect;

    #[test]
    fn acc_22_compound_commands_split_into_independent_units() {
        let cases = [
            ("rm -rf build && cargo test", 2),
            ("a | b | c", 3),
            ("cat x; echo y", 2),
            ("echo hi", 1),
        ];

        for (raw, expected) in cases {
            let analysis = analyze(raw, Path::new("/repo"));
            assert!(!analysis.unparsed, "input unexpectedly unparsed: {raw}");
            assert_eq!(analysis.units.len(), expected, "input: {raw}");
        }
    }

    #[test]
    fn acc_22_unquoted_heredoc_commands_are_independent_units() {
        let analysis = analyze("cat <<EOF\n$(whoami)\nEOF", Path::new("/repo"));

        assert!(!analysis.unparsed);
        assert_eq!(analysis.units.len(), 2);
        assert_eq!(analysis.units[1].display, "whoami");

        let quoted = analyze("cat <<'EOF'\n$(whoami)\nEOF", Path::new("/repo"));
        assert!(!quoted.unparsed);
        assert_eq!(quoted.units.len(), 1);
    }

    #[test]
    fn acc_24_supported_parser_rejection_and_syntax_failure_share_unparsed_path() {
        for raw in ["echo $(whoami)", "echo 'unterminated"] {
            let analysis = analyze(raw, Path::new("/repo"));
            assert!(analysis.unparsed, "input should be unparsed: {raw}");
            assert_eq!(analysis.units.len(), 1);
            assert_eq!(analysis.units[0].display, raw);
        }
    }

    #[test]
    fn acc_26_and_52_redirection_is_a_separate_effect() {
        let plain = analyze("cargo test", Path::new("/repo"));
        let redirected = analyze("cargo test > /tmp/log", Path::new("/repo"));

        assert!(!plain.unparsed);
        assert!(!redirected.unparsed);
        assert_eq!(plain.units[0].effects.len(), 1);
        assert_eq!(redirected.units[0].effects.len(), 2);
        assert!(
            redirected.units[0]
                .effects
                .contains(&Effect::write("/tmp/log"))
        );
    }

    #[test]
    fn acc_01_cat_and_acc_52_output_path_are_both_extracted() {
        let analysis = analyze("cat a.txt > b.txt", Path::new("/repo"));

        assert!(!analysis.unparsed);
        assert!(
            analysis.units[0]
                .effects
                .contains(&Effect::read("/repo/a.txt"))
        );
        assert!(
            analysis.units[0]
                .effects
                .contains(&Effect::write("/repo/b.txt"))
        );
    }
}
