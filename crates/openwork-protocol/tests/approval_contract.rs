use openwork_protocol::{
    approval::{
        ApprovalPolicy, ApprovalRequested, ApprovalResolution, ApprovalResolved,
        ExecutionPolicyDecision, ResolveApproval,
    },
    domain::{ActionRunId, ApprovalId, TurnId},
};
use serde_json::json;

#[test]
fn approval_contract_round_trips_with_strong_ids() {
    let requested = ApprovalRequested {
        approval_id: ApprovalId::new("approval-1"),
        turn_id: TurnId::new("turn-1"),
        action_run_id: ActionRunId::new("action-1"),
        tool_name: "bash".to_string(),
        input: json!({"command": "cargo test"}),
        reason: "process execution requires user approval".to_string(),
    };

    let encoded = serde_json::to_value(&requested).expect("approval request serializes");
    let decoded: ApprovalRequested =
        serde_json::from_value(encoded).expect("approval request deserializes");

    assert_eq!(decoded, requested);
    assert_ne!(
        requested.approval_id.as_str(),
        requested.action_run_id.as_str()
    );
}

#[test]
fn approval_resolution_is_a_recordable_fact() {
    let resolved = ApprovalResolved {
        approval_id: ApprovalId::new("approval-1"),
        turn_id: TurnId::new("turn-1"),
        action_run_id: ActionRunId::new("action-1"),
        resolution: ApprovalResolution::Allow,
    };

    let encoded = serde_json::to_value(&resolved).expect("resolution serializes");
    let decoded: ApprovalResolved =
        serde_json::from_value(encoded).expect("resolution deserializes");
    assert_eq!(decoded, resolved);
}

#[test]
fn resolve_approval_carries_turn_and_approval_identity() {
    let command = ResolveApproval {
        turn_id: TurnId::new("turn-1"),
        approval_id: ApprovalId::new("approval-1"),
        resolution: ApprovalResolution::Deny {
            reason: "denied by user".to_string(),
        },
    };

    let encoded = serde_json::to_value(&command).expect("command serializes");
    assert_eq!(encoded["turn_id"], "turn-1");
    assert_eq!(encoded["approval_id"], "approval-1");
    assert_eq!(encoded["resolution"]["type"], "deny");
}

#[test]
fn execution_policy_contract_has_only_explicit_outcomes() {
    let outcomes = [
        ExecutionPolicyDecision::Allow,
        ExecutionPolicyDecision::Deny {
            reason: "outside permitted roots".to_string(),
        },
        ExecutionPolicyDecision::RequireApproval {
            reason: "workspace mutation".to_string(),
        },
    ];

    assert_eq!(outcomes.len(), 3);
    assert_eq!(ApprovalPolicy::default(), ApprovalPolicy::Untrusted);
}
