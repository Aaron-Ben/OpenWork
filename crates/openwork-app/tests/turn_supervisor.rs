use openwork_app::{TurnSupervisor, TurnSupervisorError};
use openwork_core::ApprovalWaitOutcome;
use openwork_protocol::{
    approval::{ApprovalRequested, ApprovalResolution, ResolveApproval},
    domain::{ActionRunId, ApprovalId, TurnId},
};
use serde_json::json;
use tokio_util::sync::CancellationToken;

#[tokio::test]
async fn supervisor_routes_resolution_to_the_matching_turn() {
    let supervisor = TurnSupervisor::default();
    let turn_id = TurnId::new("turn-1");
    let mut inbox = supervisor
        .register(turn_id.clone())
        .expect("turn registers once");
    inbox
        .begin_approval(ApprovalRequested {
            approval_id: ApprovalId::new("approval-1"),
            turn_id: turn_id.clone(),
            action_run_id: ActionRunId::new("action-1"),
            tool_name: "bash".to_string(),
            input: json!({"command": "cargo test"}),
            reason: "process execution requires user approval".to_string(),
        })
        .expect("approval starts");

    let waiter =
        tokio::spawn(async move { inbox.wait_for_resolution(&CancellationToken::new()).await });
    supervisor
        .resolve(ResolveApproval {
            turn_id,
            approval_id: ApprovalId::new("approval-1"),
            resolution: ApprovalResolution::Allow,
        })
        .await
        .expect("application routes command");

    assert_eq!(
        waiter.await.expect("turn resumes"),
        ApprovalWaitOutcome::Resolved(ApprovalResolution::Allow)
    );
}

#[tokio::test]
async fn supervisor_rejects_unknown_turn() {
    let error = TurnSupervisor::default()
        .resolve(ResolveApproval {
            turn_id: TurnId::new("missing"),
            approval_id: ApprovalId::new("approval-1"),
            resolution: ApprovalResolution::Allow,
        })
        .await
        .expect_err("unknown turn is rejected");

    assert!(matches!(error, TurnSupervisorError::TurnNotFound(_)));
}
