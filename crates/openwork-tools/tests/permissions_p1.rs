use openwork_tools::{
    AnalysisUnit, AskSource, Authorization, Effect, ExecPattern, InvocationAnalysis, PathPattern,
    PermissionEngine, PermissionMode, PermissionProfile, Rule, RuleBehavior, RulePattern,
    RuleScope, ToolCallContext, ToolCallId, ToolInvocation, ToolSessionContext, ToolsetConfig,
    UnitVerdict, builtin_registry,
};
use tokio_util::sync::CancellationToken;

fn invocation(raw: &str, effect: Effect) -> InvocationAnalysis {
    InvocationAnalysis::new(raw, vec![AnalysisUnit::new(raw, vec![effect])])
}

#[test]
fn acc_10_default_asks_for_write_and_accept_edits_allows_it() {
    let engine = PermissionEngine::for_workspace("/repo");
    let analysis = invocation("write src/main.rs", Effect::write("/repo/src/main.rs"));

    assert!(matches!(
        engine.authorize(PermissionMode::Default, &analysis, &[]),
        Authorization::Ask { .. }
    ));
    assert!(matches!(
        engine.authorize(PermissionMode::AcceptEdits, &analysis, &[]),
        Authorization::Allow { .. }
    ));
}

#[test]
fn acc_12_no_mode_auto_allows_unprovable_exec() {
    let engine = PermissionEngine::for_workspace("/repo");
    let analysis = invocation(
        "cargo test",
        Effect::Exec {
            program: "cargo".to_string(),
            args: vec!["test".to_string()],
        },
    );

    for mode in [PermissionMode::Default, PermissionMode::AcceptEdits] {
        assert!(matches!(
            engine.authorize(mode, &analysis, &[]),
            Authorization::Ask { .. }
        ));
    }
}

#[test]
fn acc_06_builtin_git_and_openwork_denies_are_silent() {
    let engine = PermissionEngine::for_workspace("/repo");

    for path in ["/repo/.git/config", "/repo/.openwork/permissions.toml"] {
        let analysis = invocation("protected write", Effect::write(path));
        assert!(matches!(
            engine.authorize(PermissionMode::AcceptEdits, &analysis, &[]),
            Authorization::Deny { silent: true, .. }
        ));
    }
}

/// permissions.md §3.4 / 验收 5: built-in rules are provided by code and the
/// user cannot delete them. The only way a user rule could "remove" one is by
/// out-ranking it, and §3.3 forbids that — `deny` and `ask` always beat a
/// later `allow`, no matter the order.
#[test]
fn acc_05_user_rules_cannot_remove_builtin_rules() {
    let engine = PermissionEngine::for_workspace_with_rules(
        "/repo",
        vec![
            Rule::new(
                "user.allow.everything",
                RulePattern::Write(PathPattern::new("/repo/**").expect("valid glob")),
                RuleBehavior::Allow,
                RuleScope::Workspace,
            ),
            Rule::new(
                "user.allow.git",
                RulePattern::Write(PathPattern::new("/repo/.git/**").expect("valid glob")),
                RuleBehavior::Allow,
                RuleScope::Workspace,
            ),
        ],
    );

    for mode in [PermissionMode::Default, PermissionMode::AcceptEdits] {
        // Built-in hard deny survives a user allow aimed straight at it.
        let protected = invocation("write git config", Effect::write("/repo/.git/config"));
        assert!(
            matches!(
                engine.authorize(mode, &protected, &[]),
                Authorization::Deny { silent: true, .. }
            ),
            "builtin deny must survive a user allow in {mode:?}"
        );

        // Built-in sensitive ask likewise cannot be downgraded to allow.
        let sensitive = invocation("write dotenv", Effect::write("/repo/.env"));
        assert!(
            matches!(
                engine.authorize(mode, &sensitive, &[]),
                Authorization::Ask { .. }
            ),
            "builtin sensitive ask must survive a user allow in {mode:?}"
        );
    }
}

#[test]
fn acc_07_sensitive_files_are_readable_but_writes_ask_in_both_modes() {
    let engine = PermissionEngine::for_workspace("/repo");

    for path in [
        "/repo/.git/config",
        "/repo/.openwork/permissions.toml",
        "/repo/.env",
        "/repo/.vscode/settings.json",
    ] {
        let read = invocation("protected read", Effect::read(path));
        assert!(matches!(
            engine.authorize(PermissionMode::Default, &read, &[]),
            Authorization::Allow { .. }
        ));
    }

    for mode in [PermissionMode::Default, PermissionMode::AcceptEdits] {
        for path in ["/repo/.env", "/repo/.vscode/settings.json"] {
            let write = invocation("sensitive write", Effect::write(path));
            assert!(matches!(
                engine.authorize(mode, &write, &[]),
                Authorization::Ask { .. }
            ));
        }
        for path in ["/repo/.git/config", "/repo/.openwork/permissions.toml"] {
            let write = invocation("protected write", Effect::write(path));
            assert!(matches!(
                engine.authorize(mode, &write, &[]),
                Authorization::Deny { silent: true, .. }
            ));
        }
    }
}

#[test]
fn workspace_root_is_not_covered_by_workspace_descendant_rules() {
    let engine = PermissionEngine::for_workspace("/repo");
    let analysis = invocation("write workspace root", Effect::write("/repo"));

    assert!(matches!(
        engine.authorize(PermissionMode::AcceptEdits, &analysis, &[]),
        Authorization::Ask { .. }
    ));
}

#[test]
fn acc_03_outside_workspace_has_no_builtin_coverage() {
    let engine = PermissionEngine::for_workspace("/repo");
    let analysis = invocation("read /etc/hosts", Effect::read("/etc/hosts"));

    assert!(matches!(
        engine.authorize(PermissionMode::Default, &analysis, &[]),
        Authorization::Ask { .. }
    ));
}

#[test]
fn approval_card_serializes_the_desktop_contract_shape() {
    let engine = PermissionEngine::for_workspace("/repo");
    let analysis = invocation("write /tmp/out", Effect::write("/tmp/out"));
    let Authorization::Ask { card, .. } = engine.authorize(PermissionMode::Default, &analysis, &[])
    else {
        panic!("outside write must ask")
    };

    let value = serde_json::to_value(card).expect("serialize approval card");
    assert_eq!(value["raw"], "write /tmp/out");
    assert_eq!(value["units"][0]["outsideWorkspace"], true);
    assert_eq!(value["units"][0]["effects"][0]["certainty"], "inferred");
    assert_eq!(value["units"][0]["effects"][0]["effect"]["kind"], "write");
    assert_eq!(value["units"][0]["verdict"]["decision"], "ask");
    assert!(value["units"][0]["verdict"].get("ruleId").is_some());
    assert!(value["units"][0]["verdict"].get("rule_id").is_none());
}

#[test]
fn acc_02_strongest_rule_wins_independent_of_order() {
    let path = PathPattern::new("/repo/src/**").expect("path pattern");
    let rules = vec![
        Rule::new(
            "allow-src",
            RulePattern::Read(path.clone()),
            RuleBehavior::Allow,
            RuleScope::Workspace,
        ),
        Rule::new(
            "ask-src",
            RulePattern::Read(path.clone()),
            RuleBehavior::Ask,
            RuleScope::Workspace,
        ),
        Rule::new(
            "deny-src",
            RulePattern::Read(path),
            RuleBehavior::Deny,
            RuleScope::Workspace,
        ),
    ];
    let analysis = invocation("read src/main.rs", Effect::read("/repo/src/main.rs"));

    for ordered in [rules.clone(), rules.into_iter().rev().collect()] {
        let engine = PermissionEngine::for_workspace_with_rules("/repo", ordered);
        assert!(matches!(
            engine.authorize(PermissionMode::Default, &analysis, &[]),
            Authorization::Deny { .. }
        ));
    }
}

#[test]
fn acc_25_exec_prefix_matches_tokens_not_string_prefixes() {
    let rule = Rule::new(
        "ask-cargo-test",
        RulePattern::Exec(ExecPattern::TokenPrefix(vec![
            "cargo".into(),
            "test".into(),
        ])),
        RuleBehavior::Ask,
        RuleScope::Workspace,
    );
    let engine = PermissionEngine::for_workspace_with_rules("/repo", vec![rule]);

    let matched = invocation(
        "cargo test --lib",
        Effect::Exec {
            program: "cargo".into(),
            args: vec!["test".into(), "--lib".into()],
        },
    );
    let unmatched = invocation(
        "cargo testsuite",
        Effect::Exec {
            program: "cargo".into(),
            args: vec!["testsuite".into()],
        },
    );

    let Authorization::Ask { card, .. } = engine.authorize(PermissionMode::Default, &matched, &[])
    else {
        panic!("matched exec must ask")
    };
    assert!(matches!(
        &card.units[0].verdict,
        UnitVerdict::Ask { rule_id: Some(id), .. } if id.as_str() == "ask-cargo-test"
    ));

    let Authorization::Ask { card, .. } =
        engine.authorize(PermissionMode::Default, &unmatched, &[])
    else {
        panic!("unmatched exec must ask")
    };
    assert!(matches!(
        card.units[0].verdict,
        UnitVerdict::Ask {
            source: AskSource::NoRuleCovers,
            rule_id: None
        }
    ));
}

#[test]
fn path_glob_star_does_not_cross_directories_and_double_star_does() {
    let star = PathPattern::new("/repo/*/file").expect("star glob");
    let double_star = PathPattern::new("/repo/**/file").expect("double-star glob");

    assert!(star.is_match(std::path::Path::new("/repo/src/file")));
    assert!(!star.is_match(std::path::Path::new("/repo/src/nested/file")));
    assert!(double_star.is_match(std::path::Path::new("/repo/src/nested/file")));
}

#[tokio::test]
async fn approved_outside_path_uses_a_call_scoped_execution_permit() {
    let sandbox = tempfile::tempdir().expect("sandbox");
    let workspace = sandbox.path().join("workspace");
    let outside = sandbox.path().join("outside.txt");
    std::fs::create_dir(&workspace).expect("workspace");
    std::fs::write(&outside, "outside").expect("outside file");
    let toolset = builtin_registry()
        .finalize(
            &ToolsetConfig::from_names(["read"]),
            ToolSessionContext::local(
                workspace.clone(),
                PermissionProfile::from_builtin_rules(&workspace),
            ),
        )
        .expect("toolset");
    let invocation = ToolInvocation::new("read", serde_json::json!({ "path": outside }));
    let Authorization::Ask { permit, .. } =
        toolset.authorize(&invocation, PermissionMode::Default, &[])
    else {
        panic!("outside read must ask")
    };

    let result = toolset
        .call(
            ToolCallContext::new(ToolCallId::new("outside-read"), CancellationToken::new()),
            invocation,
            permit,
        )
        .await;
    assert!(!result.is_error(), "{}", result.text_content());
}

#[cfg(unix)]
#[tokio::test]
async fn execution_permit_does_not_allow_a_workspace_symlink_escape() {
    use std::os::unix::fs::symlink;

    let sandbox = tempfile::tempdir().expect("sandbox");
    let workspace = sandbox.path().join("workspace");
    let outside = sandbox.path().join("outside.txt");
    std::fs::create_dir(&workspace).expect("workspace");
    std::fs::write(&outside, "outside").expect("outside file");
    symlink(&outside, workspace.join("link.txt")).expect("symlink");
    let toolset = builtin_registry()
        .finalize(
            &ToolsetConfig::from_names(["read"]),
            ToolSessionContext::local(
                workspace.clone(),
                PermissionProfile::from_builtin_rules(&workspace),
            ),
        )
        .expect("toolset");
    let invocation = ToolInvocation::new("read", serde_json::json!({ "path": "link.txt" }));
    let Authorization::Allow { permit, .. } =
        toolset.authorize(&invocation, PermissionMode::Default, &[])
    else {
        panic!("lexical workspace read should be eligible for automatic execution")
    };

    let result = toolset
        .call(
            ToolCallContext::new(ToolCallId::new("symlink-read"), CancellationToken::new()),
            invocation,
            permit,
        )
        .await;
    assert!(result.is_error());
}

#[cfg(unix)]
#[tokio::test]
async fn approved_write_cannot_follow_a_workspace_symlink_outside() {
    use std::os::unix::fs::symlink;

    let sandbox = tempfile::tempdir().expect("sandbox");
    let workspace = sandbox.path().join("workspace");
    let outside = sandbox.path().join("outside.txt");
    std::fs::create_dir(&workspace).expect("workspace");
    std::fs::write(&outside, "outside").expect("outside file");
    symlink(&outside, workspace.join("link.txt")).expect("symlink");
    let toolset = builtin_registry()
        .finalize(
            &ToolsetConfig::from_names(["write"]),
            ToolSessionContext::local(
                workspace.clone(),
                PermissionProfile::from_builtin_rules(&workspace),
            ),
        )
        .expect("toolset");
    let invocation = ToolInvocation::new(
        "write",
        serde_json::json!({ "path": "link.txt", "content": "changed" }),
    );
    let Authorization::Allow { permit, .. } =
        toolset.authorize(&invocation, PermissionMode::AcceptEdits, &[])
    else {
        panic!("lexical workspace write should reach execution enforcement")
    };

    let result = toolset
        .call(
            ToolCallContext::new(ToolCallId::new("symlink-write"), CancellationToken::new()),
            invocation,
            permit,
        )
        .await;
    assert!(result.is_error());
    assert_eq!(std::fs::read_to_string(outside).unwrap(), "outside");
}

#[cfg(unix)]
#[tokio::test]
async fn creatable_deep_path_cannot_cross_an_outside_directory_symlink() {
    use std::os::unix::fs::symlink;

    let sandbox = tempfile::tempdir().expect("sandbox");
    let workspace = sandbox.path().join("workspace");
    let outside = sandbox.path().join("outside");
    std::fs::create_dir(&workspace).expect("workspace");
    std::fs::create_dir(&outside).expect("outside");
    symlink(&outside, workspace.join("linked-dir")).expect("symlink");
    let toolset = builtin_registry()
        .finalize(
            &ToolsetConfig::from_names(["write"]),
            ToolSessionContext::local(
                workspace.clone(),
                PermissionProfile::from_builtin_rules(&workspace),
            ),
        )
        .expect("toolset");
    let invocation = ToolInvocation::new(
        "write",
        serde_json::json!({
            "path": "linked-dir/deep/new.txt",
            "content": "must stay inside"
        }),
    );
    let Authorization::Allow { permit, .. } =
        toolset.authorize(&invocation, PermissionMode::AcceptEdits, &[])
    else {
        panic!("lexical workspace write should reach execution enforcement")
    };

    let result = toolset
        .call(
            ToolCallContext::new(
                ToolCallId::new("deep-symlink-write"),
                CancellationToken::new(),
            ),
            invocation,
            permit,
        )
        .await;
    assert!(result.is_error());
    assert!(!outside.join("deep/new.txt").exists());
}
