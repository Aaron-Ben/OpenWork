use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use openwork_tools::{
    AnalysisUnit, ApprovalSessionAction, Authorization, DecisionSource, Effect, ExecPattern,
    FinalizedToolset, InvocationAnalysis, LocalFileSystem, PermissionEngine, PermissionMode,
    PermissionProfile, Rule, RuleBehavior, RulePattern, RuleScope, TokioProcessBackend,
    ToolInvocation, ToolSessionContext, ToolsetConfig, UnitVerdict, builtin_registry,
    reduce_exec_grant,
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

fn authorize(toolset: &FinalizedToolset, command: &str, mode: PermissionMode) -> Authorization {
    authorize_with_rules(toolset, command, mode, &[])
}

fn authorize_with_rules(
    toolset: &FinalizedToolset,
    command: &str,
    mode: PermissionMode,
    session_rules: &[Rule],
) -> Authorization {
    toolset.authorize(
        &ToolInvocation::new("bash", serde_json::json!({ "command": command })),
        mode,
        session_rules,
    )
}

fn assert_allows_in_both_modes(toolset: &FinalizedToolset, command: &str) {
    for mode in [PermissionMode::Default, PermissionMode::AcceptEdits] {
        let authorization = authorize(toolset, command, mode);
        assert!(
            matches!(authorization, Authorization::Allow { .. }),
            "expected {command:?} to auto-allow in {mode:?}, got {authorization:?}"
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
fn acc_14_p3_daily_readonly_commands_auto_allow_in_both_modes() {
    let toolset = bash_toolset(Path::new("/repo"));

    for command in [
        "head README.md",
        "tail -n 20 app.log",
        "wc -l README.md",
        "nl README.md",
        "tac app.log",
        "rev README.md",
        "grep -n needle src",
        "basename src/main.rs",
        "dirname src/main.rs",
        "realpath src/main.rs",
        "stat README.md",
        "file README.md",
        "sort -r names.txt",
        "uniq names.txt",
        "cut -d : -f 1 names.txt",
        "tr a-z A-Z",
        "diff -u before.txt after.txt",
        "whoami",
        "id",
        "uname -a",
        "hostname",
        "date",
        "df -h .",
        "du -sh src",
        "which cargo",
        "type cargo",
        "echo hello",
        "printf hello",
        "true",
        "false",
        "seq 1 3",
        "git log --oneline",
        "git show --stat HEAD",
        "git blame src/main.rs",
        "git ls-files src",
        "git rev-parse HEAD",
        "git shortlog HEAD",
        "git merge-base HEAD main",
        "git describe HEAD",
        "git stash list",
        "git worktree list",
    ] {
        assert_allows_in_both_modes(&toolset, command);
    }
}

#[test]
fn acc_19_p3_write_and_long_running_forms_stay_unprovable() {
    let toolset = bash_toolset(Path::new("/repo"));

    for command in [
        "tail -f app.log",
        "grep -f patterns.txt src",
        "sort -o out.txt in.txt",
        "uniq in.txt out.txt",
        "hostname changed-hostname",
        "git branch -d feat",
        "git tag -d v1",
        "git reflog expire",
    ] {
        assert_asks_in_both_modes(&toolset, command);
    }
}

#[test]
fn acc_29_arity_reduction_uses_known_prefixes_and_literal_fallback() {
    let cases = [
        (
            "git",
            &["checkout", "main", "-b", "feat"][..],
            ExecPattern::TokenPrefix(vec!["git".into(), "checkout".into()]),
            false,
        ),
        (
            "npm",
            &["run", "dev", "--silent"][..],
            ExecPattern::TokenPrefix(vec!["npm".into(), "run".into(), "dev".into()]),
            false,
        ),
        (
            "git",
            &["config", "user.name", "x"][..],
            ExecPattern::TokenPrefix(vec!["git".into(), "config".into(), "user.name".into()]),
            false,
        ),
        (
            "cargo",
            &["test", "-p", "openwork-tools"][..],
            ExecPattern::TokenPrefix(vec!["cargo".into(), "test".into()]),
            false,
        ),
        (
            "someunknowntool",
            &["a", "b"][..],
            ExecPattern::Literal(vec!["someunknowntool".into(), "a".into(), "b".into()]),
            true,
        ),
    ];

    for (program, args, expected_pattern, expected_exact) in cases {
        let suggestion = reduce_exec_grant(
            program,
            &args
                .iter()
                .map(|value| (*value).to_string())
                .collect::<Vec<_>>(),
        );
        assert_eq!(suggestion.pattern, expected_pattern);
        assert_eq!(suggestion.exact, expected_exact);
    }
}

fn session_exec_rule(id: &str, pattern: ExecPattern) -> Rule {
    Rule::new(
        id,
        RulePattern::Exec(pattern),
        RuleBehavior::Allow,
        RuleScope::Session,
    )
}

#[test]
fn acc_54_and_62_exec_suggestion_becomes_a_normal_session_rule() {
    let toolset = bash_toolset(Path::new("/repo"));
    let Authorization::Ask { card, .. } = authorize(
        &toolset,
        "cargo test -p openwork-tools",
        PermissionMode::Default,
    ) else {
        panic!("unproved cargo test must ask")
    };
    let Some(ApprovalSessionAction::AllowExec { grants }) = card.session_action else {
        panic!("eligible exec should offer a session grant")
    };
    assert_eq!(grants.len(), 1);
    assert_eq!(grants[0].label, "cargo test");
    assert!(!grants[0].exact);

    let rules = vec![session_exec_rule(
        "session.tool-call-1.0",
        grants[0].pattern.clone(),
    )];
    let Authorization::Allow { evidence, .. } = authorize_with_rules(
        &toolset,
        "cargo test -p openwork-tools",
        PermissionMode::Default,
        &rules,
    ) else {
        panic!("the suggested session rule must unblock the same command")
    };
    assert_eq!(evidence.source, DecisionSource::SessionGrant);
    assert_eq!(evidence.rule_scope, Some(RuleScope::Session));
    assert_eq!(
        evidence.rule_id.as_ref().map(|id| id.as_str()),
        Some("session.tool-call-1.0")
    );
}

#[test]
fn acc_29_literal_fallback_is_explained_on_the_card() {
    let toolset = bash_toolset(Path::new("/repo"));
    let Authorization::Ask { card, .. } =
        authorize(&toolset, "custom-tool a b", PermissionMode::Default)
    else {
        panic!("unknown command must ask")
    };
    let Some(ApprovalSessionAction::AllowExec { grants }) = card.session_action else {
        panic!("eligible unknown command should offer an exact session grant")
    };
    assert_eq!(grants[0].label, "custom-tool a b");
    assert!(grants[0].exact);
}

#[test]
fn acc_32_to_37_session_grants_still_pass_through_the_eligibility_gate() {
    let toolset = bash_toolset(Path::new("/repo"));
    let cargo_grant = session_exec_rule(
        "session.cargo-test",
        ExecPattern::TokenPrefix(vec!["cargo".into(), "test".into()]),
    );
    let git_grant = session_exec_rule(
        "session.git-log",
        ExecPattern::TokenPrefix(vec!["git".into(), "log".into()]),
    );

    assert!(matches!(
        authorize_with_rules(
            &toolset,
            "cargo test -p openwork-tools",
            PermissionMode::Default,
            std::slice::from_ref(&cargo_grant),
        ),
        Authorization::Allow { .. }
    ));
    for command in [
        "PATH=. cargo test",
        "RUST_LOG=debug cargo test",
        "timeout 5 cargo test",
        "./cargo test",
        "cd /tmp && cargo test",
    ] {
        assert!(matches!(
            authorize_with_rules(
                &toolset,
                command,
                PermissionMode::Default,
                std::slice::from_ref(&cargo_grant),
            ),
            Authorization::Ask { .. }
        ));
    }
    assert!(matches!(
        authorize_with_rules(
            &toolset,
            "git -c core.pager=X log",
            PermissionMode::Default,
            std::slice::from_ref(&git_grant),
        ),
        Authorization::Ask { .. }
    ));
}

#[test]
fn acc_31_55_ineligible_or_unparsed_exec_has_no_session_button() {
    let toolset = bash_toolset(Path::new("/repo"));

    for command in [
        "python script.py",
        "PATH=. cargo test",
        "timeout 5 cargo test",
        "./cargo test",
        "echo $(whoami)",
    ] {
        let Authorization::Ask { card, .. } = authorize(&toolset, command, PermissionMode::Default)
        else {
            panic!("{command:?} must ask")
        };
        assert_eq!(card.session_action, None, "command: {command}");
    }
}

#[test]
fn acc_59_63_64_mode_suggestion_only_appears_when_it_unlocks_the_whole_call() {
    let write_toolset = builtin_registry()
        .finalize(
            &ToolsetConfig::from_names(["write"]),
            ToolSessionContext::local(
                Path::new("/repo").to_path_buf(),
                PermissionProfile::from_builtin_rules("/repo"),
            ),
        )
        .expect("write toolset");

    let ordinary = ToolInvocation::new(
        "write",
        serde_json::json!({ "path": "src/main.rs", "content": "fn main() {}" }),
    );
    let Authorization::Ask { card, .. } =
        write_toolset.authorize(&ordinary, PermissionMode::Default, &[])
    else {
        panic!("default mode must ask before a write")
    };
    assert_eq!(
        card.session_action,
        Some(ApprovalSessionAction::EnableAcceptEdits)
    );

    for path in ["/tmp/out.txt", ".env"] {
        let invocation =
            ToolInvocation::new("write", serde_json::json!({ "path": path, "content": "x" }));
        let Authorization::Ask { card, .. } =
            write_toolset.authorize(&invocation, PermissionMode::Default, &[])
        else {
            panic!("{path} must ask")
        };
        assert_eq!(card.session_action, None, "path: {path}");
    }

    let bash = bash_toolset(Path::new("/repo"));
    let Authorization::Ask { card, .. } = authorize(&bash, "mkdir src/x", PermissionMode::Default)
    else {
        panic!("default mode must still ask before mkdir")
    };
    assert_eq!(
        card.session_action,
        Some(ApprovalSessionAction::EnableAcceptEdits)
    );
}

#[test]
fn acc_55_and_70_explicit_ask_beats_a_session_grant_and_hides_the_button() {
    let analysis = InvocationAnalysis::new(
        "git push origin main",
        vec![AnalysisUnit::new(
            "git push origin main",
            vec![Effect::Exec {
                program: "git".into(),
                args: vec!["push".into(), "origin".into(), "main".into()],
            }],
        )],
    );
    let engine = PermissionEngine::for_workspace_with_rules(
        "/repo",
        vec![Rule::new(
            "user.ask.git-push",
            RulePattern::Exec(ExecPattern::TokenPrefix(vec!["git".into(), "push".into()])),
            RuleBehavior::Ask,
            RuleScope::Session,
        )],
    );
    let session_rules = vec![session_exec_rule(
        "session.git",
        ExecPattern::TokenPrefix(vec!["git".into()]),
    )];

    let Authorization::Ask { card, .. } =
        engine.authorize(PermissionMode::Default, &analysis, &session_rules)
    else {
        panic!("explicit ask must beat the session grant")
    };
    assert!(matches!(card.units[0].verdict, UnitVerdict::Ask { .. }));
    assert_eq!(card.session_action, None);
}
