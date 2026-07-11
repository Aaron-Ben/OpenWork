use openwork_core::{
    ApprovalCommandError, ApprovalState, ApprovalWaitOutcome, turn_command_channel,
};
use openwork_protocol::{
    approval::{ApprovalRequested, ApprovalResolution, ResolveApproval},
    domain::{ActionRunId, ApprovalId, TurnId},
};
use serde_json::json;
use tokio_util::sync::CancellationToken;

fn request(turn_id: &TurnId, approval_id: &str) -> ApprovalRequested {
    ApprovalRequested {
        approval_id: ApprovalId::new(approval_id),
        turn_id: turn_id.clone(),
        action_run_id: ActionRunId::new("action-1"),
        tool_name: "bash".to_string(),
        input: json!({"command": "cargo test"}),
        reason: "process execution requires user approval".to_string(),
    }
}

#[tokio::test]
async fn resolve_command_reaches_the_owning_turn_and_resumes_it() {
    let turn_id = TurnId::new("turn-1");
    let approval = request(&turn_id, "approval-1");
    let (handle, mut inbox) = turn_command_channel(turn_id.clone());

    inbox
        .begin_approval(approval)
        .expect("turn accepts its own approval request");
    let waiter = tokio::spawn(async move {
        let outcome = inbox.wait_for_resolution(&CancellationToken::new()).await;
        (outcome, inbox.into_state())
    });

    handle
        .resolve(ResolveApproval {
            turn_id,
            approval_id: ApprovalId::new("approval-1"),
            resolution: ApprovalResolution::Allow,
        })
        .await
        .expect("matching command is accepted");

    let (outcome, state) = waiter.await.expect("waiter completes");
    assert_eq!(
        outcome,
        ApprovalWaitOutcome::Resolved(ApprovalResolution::Allow)
    );
    assert!(matches!(state, ApprovalState::Resolved { .. }));
}

#[tokio::test]
async fn mismatched_approval_id_is_rejected_without_resuming_turn() {
    let turn_id = TurnId::new("turn-1");
    let approval = request(&turn_id, "approval-1");
    let (handle, mut inbox) = turn_command_channel(turn_id.clone());

    inbox
        .begin_approval(approval)
        .expect("turn accepts its own approval request");
    let waiter =
        tokio::spawn(async move { inbox.wait_for_resolution(&CancellationToken::new()).await });

    let error = handle
        .resolve(ResolveApproval {
            turn_id: turn_id.clone(),
            approval_id: ApprovalId::new("approval-other"),
            resolution: ApprovalResolution::Allow,
        })
        .await
        .expect_err("wrong approval id is rejected");
    assert!(matches!(
        error,
        ApprovalCommandError::ApprovalMismatch { .. }
    ));

    handle
        .resolve(ResolveApproval {
            turn_id,
            approval_id: ApprovalId::new("approval-1"),
            resolution: ApprovalResolution::Deny {
                reason: "denied by user".to_string(),
            },
        })
        .await
        .expect("correct approval still resolves");

    assert!(matches!(
        waiter.await.expect("waiter completes"),
        ApprovalWaitOutcome::Resolved(ApprovalResolution::Deny { .. })
    ));
}

#[tokio::test]
async fn cancellation_marks_waiting_approval_cancelled() {
    let turn_id = TurnId::new("turn-1");
    let approval = request(&turn_id, "approval-1");
    let (_handle, mut inbox) = turn_command_channel(turn_id);
    let cancel = CancellationToken::new();
    cancel.cancel();

    inbox
        .begin_approval(approval)
        .expect("turn accepts its own approval request");
    let outcome = inbox.wait_for_resolution(&cancel).await;

    assert_eq!(outcome, ApprovalWaitOutcome::Cancelled);
    assert!(matches!(
        inbox.into_state(),
        ApprovalState::Cancelled { .. }
    ));
}

#[tokio::test]
async fn concurrent_duplicate_resolution_is_rejected_before_queueing() {
    let turn_id = TurnId::new("turn-1");
    let approval = request(&turn_id, "approval-1");
    let (handle, mut inbox) = turn_command_channel(turn_id.clone());
    inbox
        .begin_approval(approval)
        .expect("turn accepts its own approval request");

    let first_handle = handle.clone();
    let first_turn_id = turn_id.clone();
    let first = tokio::spawn(async move {
        first_handle
            .resolve(ResolveApproval {
                turn_id: first_turn_id,
                approval_id: ApprovalId::new("approval-1"),
                resolution: ApprovalResolution::Allow,
            })
            .await
    });
    tokio::task::yield_now().await;

    let duplicate = tokio::time::timeout(
        std::time::Duration::from_millis(50),
        handle.resolve(ResolveApproval {
            turn_id,
            approval_id: ApprovalId::new("approval-1"),
            resolution: ApprovalResolution::Allow,
        }),
    )
    .await
    .expect("duplicate command must fail without waiting")
    .expect_err("duplicate resolution is rejected");
    assert!(matches!(
        duplicate,
        ApprovalCommandError::AlreadyResolving(_)
    ));

    assert_eq!(
        inbox.wait_for_resolution(&CancellationToken::new()).await,
        ApprovalWaitOutcome::Resolved(ApprovalResolution::Allow)
    );
    first
        .await
        .expect("first resolver task completes")
        .expect("first resolution succeeds");
}
