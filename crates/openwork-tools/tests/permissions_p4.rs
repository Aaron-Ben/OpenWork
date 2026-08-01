use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use openwork_tools::{
    ApprovalSessionAction, AskSource, Authorization, DecisionSource, Effect, ExecPattern,
    FinalizedToolset, LocalFileSystem, PathPattern, PermissionMode, PermissionProfile, Rule,
    RuleBehavior, RulePattern, RuleScope, TokioProcessBackend, ToolInvocation, ToolSessionContext,
    ToolsetConfig, UnitVerdict, builtin_registry,
};

fn bash_toolset(workspace: &Path) -> FinalizedToolset {
    let environment = Arc::new(HashMap::from([(
        "PATH".to_string(),
        "/usr/bin:/bin".to_string(),
    )]));
    builtin_registry()
        .finalize(
            &ToolsetConfig::from_names(["bash"]),
            ToolSessionContext::new(
                workspace.to_path_buf(),
                PermissionProfile::from_builtin_rules(workspace),
                environment,
                Arc::new(LocalFileSystem),
                Arc::new(TokioProcessBackend),
            ),
        )
        .expect("bash toolset")
}

fn authorize(
    toolset: &FinalizedToolset,
    command: &str,
    mode: PermissionMode,
    rules: &[Rule],
) -> Authorization {
    toolset.authorize(
        &ToolInvocation::new("bash", serde_json::json!({ "command": command })),
        mode,
        rules,
    )
}

fn exec_rule(id: &str, behavior: RuleBehavior) -> Rule {
    Rule::new(
        id,
        RulePattern::Exec(ExecPattern::TokenPrefix(vec!["echo".into()])),
        behavior,
        RuleScope::Session,
    )
}

fn write_rule(id: &str, behavior: RuleBehavior) -> Rule {
    Rule::new(
        id,
        RulePattern::Write(PathPattern::new("/repo/**").expect("valid path glob")),
        behavior,
        RuleScope::Session,
    )
}

fn assert_asks(toolset: &FinalizedToolset, command: &str, mode: PermissionMode) {
    assert!(
        matches!(
            authorize(toolset, command, mode, &[]),
            Authorization::Ask { .. }
        ),
        "expected {command:?} to ask in {mode:?}"
    );
}

fn assert_allows(toolset: &FinalizedToolset, command: &str, mode: PermissionMode) {
    assert!(
        matches!(
            authorize(toolset, command, mode, &[]),
            Authorization::Allow { .. }
        ),
        "expected {command:?} to allow in {mode:?}"
    );
}

#[test]
fn acc_27_and_28_redirection_write_is_merged_with_readonly_exec_evidence() {
    let toolset = bash_toolset(Path::new("/repo"));

    assert!(matches!(
        authorize(&toolset, "cat a.txt > b.txt", PermissionMode::Default, &[]),
        Authorization::Ask { .. }
    ));
    assert!(matches!(
        authorize(
            &toolset,
            "cat a.txt > b.txt",
            PermissionMode::AcceptEdits,
            &[]
        ),
        Authorization::Allow { .. }
    ));

    for mode in [PermissionMode::Default, PermissionMode::AcceptEdits] {
        assert!(matches!(
            authorize(&toolset, "ls > /dev/null", mode, &[]),
            Authorization::Allow { .. }
        ));
    }
}

#[test]
fn step_1_requires_allow_evidence_for_every_effect() {
    let toolset = bash_toolset(Path::new("/repo"));
    let exec_allow = exec_rule("user.allow.echo", RuleBehavior::Allow);
    let write_allow = write_rule("user.allow.workspace-write", RuleBehavior::Allow);

    assert!(matches!(
        authorize(
            &toolset,
            "echo x > src/a.txt",
            PermissionMode::Default,
            &[exec_allow.clone(), write_allow]
        ),
        Authorization::Allow { .. }
    ));

    let Authorization::Ask { card, .. } = authorize(
        &toolset,
        "echo x > src/a.txt",
        PermissionMode::Default,
        std::slice::from_ref(&exec_allow),
    ) else {
        panic!("a write without allow evidence must ask")
    };
    assert!(matches!(
        card.units[0].verdict,
        UnitVerdict::Ask {
            source: AskSource::NoRuleCovers,
            ..
        }
    ));
}

#[test]
fn step_1_rule_barriers_and_exec_eligibility_still_win() {
    let toolset = bash_toolset(Path::new("/repo"));
    let exec_allow = exec_rule("user.allow.echo", RuleBehavior::Allow);

    let Authorization::Ask { card, .. } = authorize(
        &toolset,
        "echo x > src/a.txt",
        PermissionMode::Default,
        &[
            exec_allow.clone(),
            write_rule("user.ask.write", RuleBehavior::Ask),
        ],
    ) else {
        panic!("an explicit ask must beat all allow evidence")
    };
    assert!(matches!(card.units[0].verdict, UnitVerdict::Ask { .. }));

    assert!(matches!(
        authorize(
            &toolset,
            "echo x > src/a.txt",
            PermissionMode::Default,
            &[
                exec_allow.clone(),
                write_rule("user.deny.write", RuleBehavior::Deny),
            ]
        ),
        Authorization::Deny { .. }
    ));

    assert!(matches!(
        authorize(
            &toolset,
            "PATH=. echo x > src/a.txt",
            PermissionMode::Default,
            &[
                exec_allow,
                write_rule("user.allow.write", RuleBehavior::Allow),
            ]
        ),
        Authorization::Ask { .. }
    ));
}

#[test]
fn acc_38_six_filesystem_commands_only_auto_allow_in_accept_edits() {
    let toolset = bash_toolset(Path::new("/repo"));

    for command in [
        "mkdir src/x",
        "rmdir src/x",
        "touch src/a.ts",
        "rm src/tmp.txt",
        "mv src/a.ts src/b.ts",
        "cp src/a.ts src/b.ts",
    ] {
        assert_asks(&toolset, command, PermissionMode::Default);
        assert_allows(&toolset, command, PermissionMode::AcceptEdits);
    }
}

#[test]
fn acc_39_to_43_filesystem_effects_still_obey_path_rules() {
    let toolset = bash_toolset(Path::new("/repo"));

    for mode in [PermissionMode::Default, PermissionMode::AcceptEdits] {
        assert_asks(&toolset, "mkdir ../outside/new", mode);
        assert!(matches!(
            authorize(&toolset, "rm -rf .git", mode, &[]),
            Authorization::Deny { .. }
        ));
    }
    for command in ["rm -rf .", "rm -rf ./", "rm .env"] {
        assert_asks(&toolset, command, PermissionMode::AcceptEdits);
    }

    assert_allows(
        &toolset,
        "cp src/a.ts src/b.ts",
        PermissionMode::AcceptEdits,
    );
    assert_asks(
        &toolset,
        "cp /etc/passwd src/x",
        PermissionMode::AcceptEdits,
    );
}

/// permissions.md §2.3④ / §3.4 / 验收 40: the workspace-root rule only means
/// something if it cannot be sidestepped by spelling the same destruction a
/// different way. `rm -rf .` and `rm -rf *` are one character apart and reach
/// nearly the same set of files, so they must decide the same way.
#[test]
fn acc_40_glob_write_targets_are_not_statically_resolvable() {
    let toolset = bash_toolset(Path::new("/repo"));

    for command in [
        "rm -rf *",
        "rm -rf ./*",
        "rm -rf /repo/*",
        "rm -rf src/*",
        "rm src/*.tmp",
        // `mv` records both operands as writes (§4.8), so a glob source counts.
        "mv src/*.ts src/dst",
        "cp src/a.rs src/*.bak",
        "sed -i 's/a/b/' src/*.ts",
        "echo x > src/*.txt",
        "rm src/a?.txt",
        "rm src/[ab].txt",
    ] {
        assert_asks(&toolset, command, PermissionMode::AcceptEdits);
    }

    // Reads keep the §2.3④ line: `*` never crosses a path separator, so the
    // lexical prefix still bounds where the expansion can land.
    for command in ["cat src/*.rs", "rg foo src/*.rs", "ls src/*"] {
        assert_allows(&toolset, command, PermissionMode::Default);
    }

    // A glob in `cp`'s source is a read (§8 差异 #9), so only the target is
    // constrained — unlike `mv`, which records both operands as writes.
    assert_allows(&toolset, "cp src/*.rs src/dst", PermissionMode::AcceptEdits);
}

#[test]
fn filesystem_command_flags_are_fail_closed() {
    let toolset = bash_toolset(Path::new("/repo"));

    for command in [
        "mkdir -p src/x",
        "mkdir -m 755 src/x",
        "rmdir -p src/x",
        "touch -amc src/a.ts",
        "touch -r src/reference.ts src/a.ts",
        "rm -rf src/tmp",
        "mv src/a.ts src/b.ts",
        "cp -R src/a src/b",
    ] {
        assert_allows(&toolset, command, PermissionMode::AcceptEdits);
    }

    for command in [
        "mv -t dest a b",
        "cp --target-directory=dest a b",
        "cp -T a b",
        "cp --parents a b/c",
        "mv --backup=numbered a b",
        "rm --one-file-system x",
        "mkdir -m",
    ] {
        assert_asks(&toolset, command, PermissionMode::AcceptEdits);
    }
}

#[test]
fn cp_mv_and_touch_emit_their_distinct_read_write_effects() {
    let toolset = bash_toolset(Path::new("/repo"));

    let Authorization::Allow { permit, .. } = authorize(
        &toolset,
        "cp src/a.ts src/b.ts",
        PermissionMode::AcceptEdits,
        &[],
    ) else {
        panic!("cp should be provable")
    };
    assert!(permit.effects().contains(&Effect::read("/repo/src/a.ts")));
    assert!(permit.effects().contains(&Effect::write("/repo/src/b.ts")));
    assert!(!permit.effects().contains(&Effect::write("/repo/src/a.ts")));

    let Authorization::Allow { permit, .. } = authorize(
        &toolset,
        "mv src/a.ts src/b.ts",
        PermissionMode::AcceptEdits,
        &[],
    ) else {
        panic!("mv should be provable")
    };
    assert!(permit.effects().contains(&Effect::write("/repo/src/a.ts")));
    assert!(permit.effects().contains(&Effect::write("/repo/src/b.ts")));

    let Authorization::Allow { permit, .. } = authorize(
        &toolset,
        "touch -r src/reference.ts src/a.ts",
        PermissionMode::AcceptEdits,
        &[],
    ) else {
        panic!("touch -r should be provable")
    };
    assert!(
        permit
            .effects()
            .contains(&Effect::read("/repo/src/reference.ts"))
    );
    assert!(permit.effects().contains(&Effect::write("/repo/src/a.ts")));
}

#[test]
fn acc_44_and_45_sed_uses_one_fail_closed_script_subset() {
    let toolset = bash_toolset(Path::new("/repo"));

    for command in [
        "sed -i 's/a/b/' src/a.ts",
        "sed -i 's|a|b|' src/a.ts",
        "sed -i 's#a#b#' src/a.ts",
        r"sed -i 's/a\/b/c/' src/a.ts",
        "sed -i 's/a/b/;2d' src/a.ts",
        "sed -i -e 's/a/b/' -e '2d' src/a.ts",
    ] {
        assert_allows(&toolset, command, PermissionMode::AcceptEdits);
        assert_asks(&toolset, command, PermissionMode::Default);
    }

    for command in [
        "sed -i 'w /tmp/x' src/a.ts",
        "sed -i 'e id' src/a.ts",
        "sed -i 's/a/b/;w /tmp/x' src/a.ts",
        "sed -i -f script.sed src/a.ts",
        "sed 'w /tmp/x' src/a.ts",
        "sed 'r /etc/passwd' src/a.ts",
    ] {
        assert_asks(&toolset, command, PermissionMode::AcceptEdits);
    }

    assert_allows(&toolset, "sed -n '1p' src/a.ts", PermissionMode::Default);
}

#[test]
fn acc_46_sed_backup_suffix_is_a_separate_write_effect() {
    let toolset = bash_toolset(Path::new("/repo"));
    let Authorization::Allow { permit, .. } = authorize(
        &toolset,
        "sed -i.bak 's/a/b/' src/a.ts",
        PermissionMode::AcceptEdits,
        &[],
    ) else {
        panic!("safe in-place sed with a backup should be provable")
    };

    assert!(permit.effects().contains(&Effect::write("/repo/src/a.ts")));
    assert!(
        permit
            .effects()
            .contains(&Effect::write("/repo/src/a.ts.bak"))
    );
}

#[test]
fn mode_filesystem_command_is_the_deciding_allow_source() {
    let toolset = bash_toolset(Path::new("/repo"));
    let Authorization::Allow { evidence, .. } =
        authorize(&toolset, "mkdir src/x", PermissionMode::AcceptEdits, &[])
    else {
        panic!("mkdir should be allowed in acceptEdits")
    };
    assert_eq!(evidence.source, DecisionSource::ModeFsCommand);

    let Authorization::Allow { evidence, .. } = authorize(
        &toolset,
        "cat a.txt > b.txt",
        PermissionMode::AcceptEdits,
        &[],
    ) else {
        panic!("readonly exec plus a mode write should be allowed")
    };
    assert_eq!(evidence.source, DecisionSource::Mode);
    assert_eq!(evidence.readonly_proof_key.as_deref(), Some("cat"));
}

#[test]
fn acc_47_accept_edits_does_not_auto_allow_other_write_commands() {
    let toolset = bash_toolset(Path::new("/repo"));

    for command in [
        "chmod 755 src/a",
        "chown user src/a",
        "ln src/a src/b",
        "tee src/a",
        "dd if=src/a of=src/b",
        "git commit -m message",
        "npm install",
    ] {
        assert_asks(&toolset, command, PermissionMode::AcceptEdits);
    }
}

#[test]
fn acc_64_and_65_mode_suggestion_must_unlock_the_entire_call() {
    let toolset = bash_toolset(Path::new("/repo"));
    let Authorization::Ask { card, .. } =
        authorize(&toolset, "mkdir src/x", PermissionMode::Default, &[])
    else {
        panic!("default mkdir must ask")
    };
    assert_eq!(
        card.session_action,
        Some(ApprovalSessionAction::EnableAcceptEdits)
    );

    let Authorization::Ask { card, .. } = authorize(
        &toolset,
        "rm -rf build && cargo test",
        PermissionMode::Default,
        &[],
    ) else {
        panic!("mixed filesystem and arbitrary exec must ask")
    };
    assert_eq!(card.session_action, None);
}
