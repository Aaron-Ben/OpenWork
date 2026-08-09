mod filesystem;

use std::path::{Path, PathBuf};

use tree_sitter::{Node, Parser};

use crate::policy::{lexical_normalize, path_is_within};

use super::super::{AnalysisUnit, Effect, InvocationAnalysis, ReadonlyProof};
use super::eligibility::{EligibilityInput, changes_working_directory, is_allow_eligible};
use super::readonly;

pub(crate) fn analyze(raw: &str, workspace: &Path, path: Option<&str>) -> InvocationAnalysis {
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
    if collect_units(root, raw.as_bytes(), workspace, workspace, path, &mut units).is_err()
        || units.is_empty()
    {
        return InvocationAnalysis::unparsed(raw);
    }

    let rebased = flat_top_level_units(root).and_then(|nodes| {
        let cd_target = safe_cd_prefix_target(&nodes, &units, workspace)?;
        let mut rebased = Vec::new();
        for (index, node) in nodes.into_iter().enumerate() {
            let before = rebased.len();
            let effect_base = if index == 0 {
                workspace
            } else {
                cd_target.as_path()
            };
            collect_units(
                node,
                raw.as_bytes(),
                workspace,
                effect_base,
                path,
                &mut rebased,
            )
            .ok()?;
            if rebased.len() != before + 1 {
                return None;
            }
        }

        // `cd` 不进入通用只读表。只有顶层结构、唯一性和目标边界都已证明后，才把
        // 这个 shell 内建命令标成结构性只读，并显式保留目标目录的 read 效果。
        let cd = rebased.first_mut()?;
        cd.effects.push(Effect::read(cd_target));
        cd.readonly_proof = Some(ReadonlyProof {
            key: "cd".to_string(),
        });
        Some(rebased)
    });

    if let Some(rebased) = rebased {
        units = rebased;
    } else if units.iter().any(|unit| {
        unit.effects.iter().any(|effect| match effect {
            Effect::Exec { program, args } => changes_working_directory(program, args),
            _ => false,
        })
    }) {
        for unit in &mut units {
            unit.allow_eligible = false;
        }
    }
    InvocationAnalysis::new(raw, units)
}

fn flat_top_level_units<'tree>(root: Node<'tree>) -> Option<Vec<Node<'tree>>> {
    if root.kind() != "program" {
        return None;
    }
    let mut units = Vec::new();
    collect_flat_program(root, &mut units).ok()?;
    Some(units)
}

fn collect_flat_program<'tree>(node: Node<'tree>, units: &mut Vec<Node<'tree>>) -> Result<(), ()> {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.is_named() {
            collect_flat_statement(child, units)?;
        } else if child.kind() != ";" {
            return Err(());
        }
    }
    Ok(())
}

fn collect_flat_statement<'tree>(
    node: Node<'tree>,
    units: &mut Vec<Node<'tree>>,
) -> Result<(), ()> {
    match node.kind() {
        "command" => {
            units.push(node);
            Ok(())
        }
        "redirected_statement"
            if node
                .child_by_field_name("body")
                .is_some_and(|body| body.kind() == "command") =>
        {
            units.push(node);
            Ok(())
        }
        "list" => {
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                if child.is_named() {
                    collect_flat_statement(child, units)?;
                } else if child.kind() != "&&" {
                    return Err(());
                }
            }
            Ok(())
        }
        _ => Err(()),
    }
}

fn safe_cd_prefix_target(
    nodes: &[Node<'_>],
    units: &[AnalysisUnit],
    workspace: &Path,
) -> Option<PathBuf> {
    if nodes.len() < 2 || nodes.len() != units.len() || nodes.first()?.kind() != "command" {
        return None;
    }
    let cwd_changes = units
        .iter()
        .flat_map(|unit| unit.effects.iter())
        .filter(|effect| match effect {
            Effect::Exec { program, args } => changes_working_directory(program, args),
            Effect::Read { .. } | Effect::Write { .. } => false,
        })
        .count();
    if cwd_changes != 1 {
        return None;
    }
    let [Effect::Exec { program, args }] = units.first()?.effects.as_slice() else {
        return None;
    };
    let [target] = args.as_slice() else {
        return None;
    };
    if program != "cd" || target == "-" || target.starts_with('~') {
        return None;
    }

    let target = resolve_effect_path(workspace, target);
    path_is_within(&target, workspace).then_some(target)
}

fn collect_units(
    node: Node<'_>,
    source: &[u8],
    workspace: &Path,
    effect_base: &Path,
    path: Option<&str>,
    units: &mut Vec<AnalysisUnit>,
) -> Result<(), ()> {
    match node.kind() {
        "program" | "list" | "pipeline" | "subshell" => {
            let mut cursor = node.walk();
            for child in node.named_children(&mut cursor) {
                collect_units(child, source, workspace, effect_base, path, units)?;
            }
            Ok(())
        }
        "command" => {
            units.push(analyze_command(node, source, workspace, effect_base, path)?);
            Ok(())
        }
        "redirected_statement" => {
            let body = node.child_by_field_name("body").ok_or(())?;
            let before = units.len();
            collect_units(body, source, workspace, effect_base, path, units)?;
            if units.len() != before + 1 {
                return Err(());
            }
            let (effects, heredoc_units) =
                analyze_redirects(node, source, workspace, effect_base, path)?;
            units[before].effects.extend(effects);
            units[before].display = node_text(node, source)?.to_string();
            units.extend(heredoc_units);
            Ok(())
        }
        "comment" => Ok(()),
        _ => Err(()),
    }
}

fn analyze_command(
    node: Node<'_>,
    source: &[u8],
    workspace: &Path,
    effect_base: &Path,
    path: Option<&str>,
) -> Result<AnalysisUnit, ()> {
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
    let proof = readonly::prove(&program, &args, effect_base);
    if let Some(proof) = &proof {
        effects.extend(proof.effects.iter().cloned());
    }
    let filesystem_proof = filesystem::prove(&program, &args, effect_base);
    if let Some(proof) = &filesystem_proof {
        effects.extend(proof.effects.iter().cloned());
    }
    let (redirection_effects, heredoc_units) =
        analyze_redirects(node, source, workspace, effect_base, path)?;
    if !heredoc_units.is_empty() {
        return Err(());
    }
    effects.extend(redirection_effects);

    let has_leading_assignment = has_leading_assignment(node);
    let allow_eligible = is_allow_eligible(EligibilityInput {
        program: &program,
        args: &args,
        workspace,
        path,
        has_leading_assignment,
    });
    Ok(AnalysisUnit::with_exec_evidence(
        node_text(node, source)?,
        effects,
        allow_eligible,
        proof.map(|proof| proof.marker),
        filesystem_proof.is_some(),
    ))
}

fn has_leading_assignment(node: Node<'_>) -> bool {
    let mut cursor = node.walk();
    node.named_children(&mut cursor)
        .any(|child| child.kind() == "variable_assignment")
}

fn analyze_redirects(
    node: Node<'_>,
    source: &[u8],
    workspace: &Path,
    effect_base: &Path,
    path: Option<&str>,
) -> Result<(Vec<Effect>, Vec<AnalysisUnit>), ()> {
    let mut effects = Vec::new();
    let mut heredoc_units = Vec::new();
    let mut cursor = node.walk();
    for redirect in node.children_by_field_name("redirect", &mut cursor) {
        match redirect.kind() {
            "file_redirect" => {
                effects.extend(file_redirect_effects(redirect, source, effect_base)?);
            }
            "heredoc_redirect" => {
                collect_heredoc_units(
                    redirect,
                    source,
                    workspace,
                    effect_base,
                    path,
                    &mut heredoc_units,
                )?;
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
    if raw.contains("<>") {
        let write = filesystem::resolve_write_path(workspace, &destination).ok_or(())?;
        return Ok(vec![Effect::read(write.clone()), Effect::write(write)]);
    }
    if raw.contains('>') {
        // A glob as a redirection target is not a statically resolvable path
        // (§2.3④). Fail closed rather than record the pattern as if it were one.
        let write = filesystem::resolve_write_path(workspace, &destination).ok_or(())?;
        return Ok(vec![Effect::write(write)]);
    }
    if raw.contains('<') {
        return Ok(vec![Effect::read(resolve_effect_path(
            workspace,
            &destination,
        ))]);
    }
    Err(())
}

fn collect_heredoc_units(
    redirect: Node<'_>,
    source: &[u8],
    workspace: &Path,
    effect_base: &Path,
    path: Option<&str>,
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
                            collect_units(command, source, workspace, effect_base, path, units)?;
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
        "word" | "number" => decode_word(node_text(node, source)?),
        "raw_string" => strip_quotes(node_text(node, source)?, '\''),
        "string" => {
            let mut cursor = node.walk();
            for child in node.named_children(&mut cursor) {
                if child.kind() != "string_content" {
                    return Err(());
                }
            }
            let raw = node_text(node, source)?;
            let inner = raw
                .strip_prefix('"')
                .and_then(|value| value.strip_suffix('"'))
                .ok_or(())?;
            decode_double_quoted_content(inner)
        }
        _ => Err(()),
    }
}

/// 双引号里的反斜杠规则比无引号 word 更窄。若把其余反斜杠也剥掉，权限分析得到的
/// 路径就会与 shell 实际访问的路径分叉，因此这里不能复用 `decode_word`。
fn decode_double_quoted_content(raw: &str) -> Result<String, ()> {
    let mut decoded = String::new();
    let mut chars = raw.chars().peekable();
    while let Some(character) = chars.next() {
        if character != '\\' {
            decoded.push(character);
            continue;
        }

        let escaped = chars.next().ok_or(())?;
        match escaped {
            '$' | '`' | '"' | '\\' => decoded.push(escaped),
            '\n' => {}
            '\r' if chars.peek() == Some(&'\n') => {
                chars.next();
            }
            other => {
                decoded.push('\\');
                decoded.push(other);
            }
        }
    }
    Ok(decoded)
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
            let analysis = analyze(raw, Path::new("/repo"), Some("/usr/bin:/bin"));
            assert!(!analysis.unparsed, "input unexpectedly unparsed: {raw}");
            assert_eq!(analysis.units.len(), expected, "input: {raw}");
        }
    }

    #[test]
    fn acc_22_unquoted_heredoc_commands_are_independent_units() {
        let analysis = analyze(
            "cat <<EOF\n$(whoami)\nEOF",
            Path::new("/repo"),
            Some("/usr/bin:/bin"),
        );

        assert!(!analysis.unparsed);
        assert_eq!(analysis.units.len(), 2);
        assert_eq!(analysis.units[1].display, "whoami");

        let quoted = analyze(
            "cat <<'EOF'\n$(whoami)\nEOF",
            Path::new("/repo"),
            Some("/usr/bin:/bin"),
        );
        assert!(!quoted.unparsed);
        assert_eq!(quoted.units.len(), 1);
    }

    #[test]
    fn acc_24_supported_parser_rejection_and_syntax_failure_share_unparsed_path() {
        for raw in ["echo $(whoami)", "echo 'unterminated"] {
            let analysis = analyze(raw, Path::new("/repo"), Some("/usr/bin:/bin"));
            assert!(analysis.unparsed, "input should be unparsed: {raw}");
            assert_eq!(analysis.units.len(), 1);
            assert_eq!(analysis.units[0].display, raw);
        }
    }

    #[test]
    fn acc_26_and_52_redirection_is_a_separate_effect() {
        let plain = analyze("cargo test", Path::new("/repo"), Some("/usr/bin:/bin"));
        let redirected = analyze(
            "cargo test > /tmp/log",
            Path::new("/repo"),
            Some("/usr/bin:/bin"),
        );

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
        let analysis = analyze(
            "cat a.txt > b.txt",
            Path::new("/repo"),
            Some("/usr/bin:/bin"),
        );

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

    #[test]
    fn quoted_literal_paths_are_provably_readonly() {
        for raw in ["ls -la \"packages/tui/src\"", "ls -la 'packages/tui/src'"] {
            let analysis = analyze(raw, Path::new("/repo"), Some("/usr/bin:/bin"));

            assert!(!analysis.unparsed, "input unexpectedly unparsed: {raw}");
            assert_eq!(
                analysis.units[0]
                    .readonly_proof
                    .as_ref()
                    .map(|proof| proof.key.as_str()),
                Some("ls"),
                "input should have a readonly proof: {raw}"
            );
        }
    }

    #[test]
    fn double_quoted_literal_paths_follow_bash_escape_rules() {
        let cases = [
            (r#"cat "a b.txt""#, "/repo/a b.txt"),
            (r#"cat "a\"b.txt""#, r#"/repo/a"b.txt"#),
            (r#"cat "a\b.txt""#, r"/repo/a\b.txt"),
        ];

        for (raw, expected_path) in cases {
            let analysis = analyze(raw, Path::new("/repo"), Some("/usr/bin:/bin"));

            assert!(!analysis.unparsed, "input unexpectedly unparsed: {raw}");
            assert!(
                analysis.units[0]
                    .effects
                    .contains(&Effect::read(expected_path)),
                "input resolved to the wrong read effect: {raw}"
            );
        }
    }

    #[test]
    fn double_quoted_expansions_remain_unparsed() {
        for raw in [r#"echo "$HOME""#, r#"cat "$(ls)""#, r#"echo "${x}""#] {
            let analysis = analyze(raw, Path::new("/repo"), Some("/usr/bin:/bin"));
            assert!(analysis.unparsed, "input should remain unparsed: {raw}");
        }
    }

    #[test]
    fn empty_double_quoted_argument_keeps_its_existing_behavior() {
        let analysis = analyze(r#"cat """#, Path::new("/repo"), Some("/usr/bin:/bin"));

        assert!(!analysis.unparsed);
        assert_eq!(
            analysis.units[0]
                .readonly_proof
                .as_ref()
                .map(|proof| proof.key.as_str()),
            Some("cat")
        );
    }
}
