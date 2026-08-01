use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use openwork_tools::{
    AnalysisUnit, AskSource, Authorization, Effect, ExecPattern, FinalizedToolset,
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
            RuleScope::Workspace,
        )],
    );
    let Authorization::Ask { card, .. } = ask.authorize(PermissionMode::Default, &analysis) else {
        panic!("explicit ask must beat readonly proof")
    };
    assert!(matches!(
        card.units[0].verdict,
        UnitVerdict::Ask {
            source: AskSource::ExplicitRule,
            ..
        }
    ));

    let deny = PermissionEngine::for_workspace_with_rules(
        "/repo",
        vec![Rule::new(
            "user.deny.git-status",
            exec(),
            RuleBehavior::Deny,
            RuleScope::Workspace,
        )],
    );
    assert!(matches!(
        deny.authorize(PermissionMode::Default, &analysis),
        Authorization::Deny { .. }
    ));

    let allow = PermissionEngine::for_workspace_with_rules(
        "/repo",
        vec![Rule::new(
            "user.allow.git-status",
            exec(),
            RuleBehavior::Allow,
            RuleScope::Workspace,
        )],
    );
    assert!(matches!(
        allow.authorize(PermissionMode::Default, &analysis),
        Authorization::Ask { .. }
    ));
}

#[test]
fn acc_28_and_50_readonly_proof_is_visible_and_does_not_hide_redirection() {
    let toolset = bash_toolset(Path::new("/repo"), "/usr/bin:/bin");

    assert_allows_in_both_modes(&toolset, "ls > /dev/null");
    assert_asks_in_both_modes(&toolset, "cat a.txt > b.txt");

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
