use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use openwork_tools::{
    AnalysisUnit, AskSource, Authorization, DecisionSource, Effect, ExecPattern, FinalizedToolset,
    InvocationAnalysis, LocalFileSystem, PermissionEngine, PermissionMode, PermissionProfile,
    ReadonlyProof, Rule, RuleBehavior, RulePattern, RuleScope, TokioProcessBackend, ToolInvocation,
    ToolSessionContext, ToolsetConfig, UnitVerdict, builtin_registry,
};

fn bash_toolset(workspace: &Path, path: &str) -> FinalizedToolset {
    let environment = Arc::new(HashMap::from([("PATH".to_string(), path.to_string())]));
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

fn authorize(toolset: &FinalizedToolset, command: &str, mode: PermissionMode) -> Authorization {
    toolset.authorize(
        &ToolInvocation::new("bash", serde_json::json!({ "command": command })),
        mode,
        &[],
    )
}

fn assert_allows_in_both_modes(toolset: &FinalizedToolset, command: &str) {
    for mode in [PermissionMode::Default, PermissionMode::AcceptEdits] {
        assert!(
            matches!(
                authorize(toolset, command, mode),
                Authorization::Allow { .. }
            ),
            "expected {command:?} to auto-allow in {mode:?}"
        );
    }
}

fn assert_asks_in_both_modes(toolset: &FinalizedToolset, command: &str) {
    for mode in [PermissionMode::Default, PermissionMode::AcceptEdits] {
        assert!(
            matches!(authorize(toolset, command, mode), Authorization::Ask { .. }),
            "expected {command:?} to ask in {mode:?}"
        );
    }
}

fn permit_effects(authorization: &Authorization) -> &[Effect] {
    match authorization {
        Authorization::Allow { permit, .. } | Authorization::Ask { permit, .. } => permit.effects(),
        Authorization::Deny { .. } | Authorization::Unavailable { .. } => {
            panic!("authorization has no inspectable permit")
        }
    }
}

#[test]
fn acc_11_and_14_readonly_commands_auto_allow_in_both_modes() {
    let toolset = bash_toolset(Path::new("/repo"), "/usr/bin:/bin");

    for command in [
        "pwd",
        "ls src",
        "cat README.md",
        "rg foo src",
        "git status",
        "git diff",
    ] {
        assert_allows_in_both_modes(&toolset, command);
    }
}

#[test]
fn acc_15_readonly_proof_does_not_bypass_path_rules() {
    let toolset = bash_toolset(Path::new("/repo"), "/usr/bin:/bin");

    assert_asks_in_both_modes(&toolset, "cat /etc/passwd");
    assert_asks_in_both_modes(&toolset, "ls --color /etc");
}

#[test]
fn acc_16_flag_arity_mismatch_cannot_smuggle_a_long_option() {
    let toolset = bash_toolset(Path::new("/repo"), "/usr/bin:/bin");

    assert_asks_in_both_modes(&toolset, "git diff -S -- --output=/tmp/pwned");
}

#[test]
fn acc_17_escape_flags_are_not_readonly() {
    let toolset = bash_toolset(Path::new("/repo"), "/usr/bin:/bin");

    for command in [
        "rg . --pre=bash FILE",
        "git -c core.pager=X status",
        "timeout 5 ls",
    ] {
        assert_asks_in_both_modes(&toolset, command);
    }
}

#[test]
fn acc_18_and_20_dynamic_tokens_and_unknown_flags_are_not_readonly() {
    let toolset = bash_toolset(Path::new("/repo"), "/usr/bin:/bin");

    for command in [
        r#"git diff "$Z--output=/tmp/x""#,
        "ls --some-unknown-flag",
        "rg 'foo\nbar' src",
        "ls file{1,2}",
        "ls `pwd`",
        // A leading `~` is a prefix rewrite, not an in-place expansion: read
        // literally it resolves inside the workspace while bash reads $HOME.
        "cat ~/.ssh/id_rsa",
        "cat ~/.aws/credentials",
        "ls ~root",
        "ls ~/",
    ] {
        assert_asks_in_both_modes(&toolset, command);
    }
}

#[test]
fn double_quoted_dollar_literals_are_preserved_before_dynamic_token_checks() {
    let toolset = bash_toolset(Path::new("/repo"), "/usr/bin:/bin");

    for (command, expected_argument) in [
        (r#"cat "a$.txt""#, "a$.txt"),
        (r#"cat "a[$]b.txt""#, "a[$]b.txt"),
        (r#"cat "a\$b.txt""#, "a$b.txt"),
    ] {
        let authorization = authorize(&toolset, command, PermissionMode::Default);
        assert!(
            matches!(&authorization, Authorization::Ask { .. }),
            "dynamic token should ask: {command}"
        );
        assert!(
            permit_effects(&authorization).contains(&Effect::Exec {
                program: "cat".to_string(),
                args: vec![expected_argument.to_string()],
            }),
            "decoded argument diverged from bash: {command}"
        );
    }

    assert_asks_in_both_modes(&toolset, r#"cat "a$b.txt""#);
}

#[test]
fn static_double_quoted_literals_keep_their_existing_authorization_and_paths() {
    let toolset = bash_toolset(Path::new("/repo"), "/usr/bin:/bin");

    for command in [r#"ls -la "src""#, r#"cat """#] {
        assert_allows_in_both_modes(&toolset, command);
    }

    for (command, expected_path) in [
        (r#"cat "a\"b.txt""#, r#"/repo/a"b.txt"#),
        (r#"cat "a\b.txt""#, r"/repo/a\b.txt"),
    ] {
        let authorization = authorize(&toolset, command, PermissionMode::Default);
        assert!(matches!(&authorization, Authorization::Allow { .. }));
        assert!(
            permit_effects(&authorization).contains(&Effect::read(expected_path)),
            "decoded path diverged from bash: {command}"
        );
    }
}

/// permissions.md §1.3: `default` only earns its keep if everyday commands stop
/// interrupting. `ls -la` and `ls -l -a` are the same call and must decide the
/// same way — but a cluster containing a value-taking flag stays unprovable.
#[test]
fn acc_14_short_flag_clusters_decide_like_their_expanded_form() {
    let toolset = bash_toolset(Path::new("/repo"), "/usr/bin:/bin");

    for command in [
        "ls -la src",
        "ls -l -a src",
        "ls -lAh src",
        "cat -nE src/a.rs",
    ] {
        assert_allows_in_both_modes(&toolset, command);
    }
    for command in ["ls -laz src", "git diff -Sx"] {
        assert_asks_in_both_modes(&toolset, command);
    }
}

#[test]
fn acc_19_write_subcommands_are_not_in_the_readonly_table() {
    let toolset = bash_toolset(Path::new("/repo"), "/usr/bin:/bin");

    for command in ["git reflog expire", "git tag -d release", "git commit"] {
        assert_asks_in_both_modes(&toolset, command);
    }
}

#[test]
fn acc_32_34_and_35_assignments_wrappers_and_cwd_changes_are_ineligible() {
    let toolset = bash_toolset(Path::new("/repo"), "/usr/bin:/bin");

    for command in [
        "PATH=. ls",
        "RUST_LOG=debug ls",
        "timeout 5 ls",
        "env -C /tmp ls",
        "cd /tmp && ls",
        "pushd /tmp && ls",
    ] {
        assert_asks_in_both_modes(&toolset, command);
    }

    let Authorization::Ask { card, .. } = authorize(&toolset, "PATH=. ls", PermissionMode::Default)
    else {
        panic!("leading assignment must ask")
    };
    assert!(!card.unparsed, "leading assignments are supported syntax");
    assert_eq!(card.units[0].display, "PATH=. ls");
}

#[test]
fn a_static_workspace_cd_prefix_rebases_following_read_effects() {
    let toolset = bash_toolset(Path::new("/repo"), "/usr/bin:/bin");

    for command in [
        r#"cd "/repo" && cat AGENTS.md"#,
        r#"cd "/repo"; cat AGENTS.md"#,
    ] {
        assert_allows_in_both_modes(&toolset, command);
    }

    let authorization = authorize(
        &toolset,
        r#"cd "/repo/packages" && ls ai"#,
        PermissionMode::Default,
    );
    assert!(matches!(&authorization, Authorization::Allow { .. }));
    assert!(
        permit_effects(&authorization).contains(&Effect::read("/repo/packages/ai")),
        "relative read should resolve from the static cd target"
    );
    assert!(!permit_effects(&authorization).contains(&Effect::read("/repo/ai")));
}

#[test]
fn cwd_changes_outside_the_narrow_static_workspace_prefix_remain_unprovable() {
    let toolset = bash_toolset(Path::new("/repo"), "/usr/bin:/bin");

    for command in [
        "cd /etc && cat passwd",
        r#"cd "/repo" && cd /etc && cat passwd"#,
        r#"cd "/repo" && pushd packages && cat AGENTS.md"#,
        r#"cd "/repo" && popd && cat AGENTS.md"#,
        r#"cd "/repo" && env -C packages cat AGENTS.md"#,
        r#"cd "/repo" && cat ../../etc/passwd"#,
        "cd $HOME && ls",
        "cd - && ls",
        "cd && ls",
        r#"(cd "/repo" && ls)"#,
        r#"cd "/repo" && rm -rf x"#,
        r#"cd "/repo" && git -C packages status"#,
    ] {
        assert!(
            matches!(
                authorize(&toolset, command, PermissionMode::Default),
                Authorization::Ask { .. }
            ),
            "cwd-changing command should remain fail-closed: {command}"
        );
    }
}

#[test]
fn acc_36_workspace_resident_executables_are_ineligible() {
    let safe_path = bash_toolset(Path::new("/repo"), "/usr/bin:/bin");
    let workspace_path = bash_toolset(Path::new("/repo"), "/usr/bin:/repo/bin");
    let relative_path = bash_toolset(Path::new("/repo"), "/usr/bin:bin");

    assert_asks_in_both_modes(&safe_path, "./ls");
    assert_asks_in_both_modes(&workspace_path, "ls");
    assert_asks_in_both_modes(&relative_path, "ls");
}

#[test]
fn acc_37_ineligibility_does_not_disable_ask_or_deny_rules() {
    let toolset = bash_toolset(Path::new("/repo"), "/usr/bin:/bin");

    let Authorization::Ask { card, .. } = authorize(
        &toolset,
        "PATH=. printf x > .env",
        PermissionMode::AcceptEdits,
    ) else {
        panic!("sensitive-path ask must survive ineligibility")
    };
    assert!(matches!(
        card.units[0].verdict,
        UnitVerdict::Ask {
            source: AskSource::BuiltinSensitive,
            ..
        }
    ));

    assert!(matches!(
        authorize(
            &toolset,
            "PATH=. printf x > .git/config",
            PermissionMode::AcceptEdits,
        ),
        Authorization::Deny { silent: true, .. }
    ));
}

#[test]
fn acc_37_explicit_exec_rules_precede_readonly_proof() {
    let analysis = InvocationAnalysis::new(
        "git status",
        vec![AnalysisUnit {
            display: "git status".to_string(),
            effects: vec![
                Effect::Exec {
                    program: "git".to_string(),
                    args: vec!["status".to_string()],
                },
                Effect::read("/repo"),
            ],
            allow_eligible: true,
            readonly_proof: Some(ReadonlyProof {
                key: "git status".to_string(),
            }),
            filesystem_command_proof: false,
        }],
    );
    let exec = || {
        RulePattern::Exec(ExecPattern::TokenPrefix(vec![
            "git".to_string(),
            "status".to_string(),
        ]))
    };

    let ask = PermissionEngine::for_workspace_with_rules(
        "/repo",
        vec![Rule::new(
            "user.ask.git-status",
            exec(),
            RuleBehavior::Ask,
            RuleScope::Session,
        )],
    );
    let Authorization::Ask { card, .. } = ask.authorize(PermissionMode::Default, &analysis, &[])
    else {
        panic!("explicit ask must beat readonly proof")
    };
    assert!(matches!(card.units[0].verdict, UnitVerdict::Ask { .. }));

    let deny = PermissionEngine::for_workspace_with_rules(
        "/repo",
        vec![Rule::new(
            "session.deny.git-status",
            exec(),
            RuleBehavior::Deny,
            RuleScope::Session,
        )],
    );
    assert!(matches!(
        deny.authorize(PermissionMode::Default, &analysis, &[]),
        Authorization::Deny { .. }
    ));

    let allow = PermissionEngine::for_workspace_with_rules(
        "/repo",
        vec![Rule::new(
            "session.allow.git-status",
            exec(),
            RuleBehavior::Allow,
            RuleScope::Session,
        )],
    );
    let Authorization::Allow { evidence, .. } =
        allow.authorize(PermissionMode::Default, &analysis, &[])
    else {
        panic!("P3 removes the legacy guard that downgraded explicit exec allow rules")
    };
    assert_eq!(evidence.source, DecisionSource::SessionGrant);
    assert_eq!(
        evidence.rule_id.as_ref().map(|id| id.as_str()),
        Some("session.allow.git-status")
    );
}

#[test]
fn acc_27_28_and_50_readonly_proof_is_visible_and_does_not_hide_redirection() {
    let toolset = bash_toolset(Path::new("/repo"), "/usr/bin:/bin");

    assert_allows_in_both_modes(&toolset, "ls > /dev/null");
    assert!(matches!(
        authorize(&toolset, "cat a.txt > b.txt", PermissionMode::Default),
        Authorization::Ask { .. }
    ));
    assert!(matches!(
        authorize(&toolset, "cat a.txt > b.txt", PermissionMode::AcceptEdits),
        Authorization::Allow { .. }
    ));

    let Authorization::Ask { card, .. } =
        authorize(&toolset, "ls && cargo test", PermissionMode::Default)
    else {
        panic!("mixed readonly and unprovable exec must ask")
    };
    let value = serde_json::to_value(card).expect("serialize approval card");
    assert_eq!(
        value["units"][0]["effects"][0]["certainty"],
        "readonly_proof"
    );
    assert_eq!(value["units"][0]["effects"][0]["key"], "ls");
    assert_eq!(value["units"][0]["verdict"]["source"], "readonly_proof");
    assert_eq!(
        value["units"][1]["effects"][0]["certainty"],
        "trusted_program"
    );
}
