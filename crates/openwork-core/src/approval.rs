use std::sync::{Arc, Mutex};

use openwork_protocol::{
    approval::{ApprovalRequested, ApprovalResolution, ResolveApproval},
    domain::{ApprovalId, TurnId},
};
use thiserror::Error;
use tokio::sync::{mpsc, oneshot};
use tokio_util::sync::CancellationToken;

const TURN_COMMAND_CAPACITY: usize = 8;

#[derive(Debug, Clone, PartialEq)]
pub enum ApprovalState {
    Idle,
    Waiting {
        request: ApprovalRequested,
    },
    Resolved {
        request: ApprovalRequested,
        resolution: ApprovalResolution,
    },
    Cancelled {
        request: ApprovalRequested,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApprovalWaitOutcome {
    Resolved(ApprovalResolution),
    Cancelled,
    CommandChannelClosed,
    StateUnavailable,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ApprovalCommandError {
    #[error("approval command targets turn {actual}, expected {expected}")]
    TurnMismatch { expected: TurnId, actual: TurnId },
    #[error("turn is not waiting for an approval")]
    NotWaiting,
    #[error("approval is already being resolved: {0}")]
    AlreadyResolving(ApprovalId),
    #[error("approval command targets {actual}, expected {expected}")]
    ApprovalMismatch {
        expected: ApprovalId,
        actual: ApprovalId,
    },
    #[error("turn command channel is closed")]
    TurnClosed,
    #[error("approval routing state is unavailable")]
    StateUnavailable,
}

#[derive(Debug, Clone)]
enum PendingApprovalRoute {
    Idle,
    Waiting(ApprovalId),
    Resolving(ApprovalId),
}

struct TurnCommandEnvelope {
    command: ResolveApproval,
    acknowledgement: oneshot::Sender<Result<(), ApprovalCommandError>>,
}

/// Application-facing handle used only to route commands to one owning Turn.
#[derive(Clone)]
pub struct TurnCommandHandle {
    turn_id: TurnId,
    sender: mpsc::Sender<TurnCommandEnvelope>,
    pending_approval: Arc<Mutex<PendingApprovalRoute>>,
}

impl TurnCommandHandle {
    pub fn turn_id(&self) -> &TurnId {
        &self.turn_id
    }

    pub async fn resolve(&self, command: ResolveApproval) -> Result<(), ApprovalCommandError> {
        if command.turn_id != self.turn_id {
            return Err(ApprovalCommandError::TurnMismatch {
                expected: self.turn_id.clone(),
                actual: command.turn_id,
            });
        }

        {
            let mut pending = self
                .pending_approval
                .lock()
                .map_err(|_| ApprovalCommandError::StateUnavailable)?;
            match &*pending {
                PendingApprovalRoute::Idle => return Err(ApprovalCommandError::NotWaiting),
                PendingApprovalRoute::Waiting(expected) => {
                    if command.approval_id != *expected {
                        return Err(ApprovalCommandError::ApprovalMismatch {
                            expected: expected.clone(),
                            actual: command.approval_id,
                        });
                    }
                }
                PendingApprovalRoute::Resolving(expected) => {
                    if command.approval_id == *expected {
                        return Err(ApprovalCommandError::AlreadyResolving(expected.clone()));
                    }
                    return Err(ApprovalCommandError::ApprovalMismatch {
                        expected: expected.clone(),
                        actual: command.approval_id,
                    });
                }
            }
            *pending = PendingApprovalRoute::Resolving(command.approval_id.clone());
        }

        let (acknowledgement, response) = oneshot::channel();
        self.sender
            .send(TurnCommandEnvelope {
                command,
                acknowledgement,
            })
            .await
            .map_err(|_| ApprovalCommandError::TurnClosed)?;
        response
            .await
            .unwrap_or(Err(ApprovalCommandError::TurnClosed))
    }
}

/// Core-owned command inbox and explicit approval state for one active Turn.
pub struct TurnCommandInbox {
    turn_id: TurnId,
    receiver: mpsc::Receiver<TurnCommandEnvelope>,
    pending_approval: Arc<Mutex<PendingApprovalRoute>>,
    state: ApprovalState,
}

impl TurnCommandInbox {
    pub fn begin_approval(
        &mut self,
        request: ApprovalRequested,
    ) -> Result<(), ApprovalCommandError> {
        if request.turn_id != self.turn_id {
            return Err(ApprovalCommandError::TurnMismatch {
                expected: self.turn_id.clone(),
                actual: request.turn_id,
            });
        }

        *self
            .pending_approval
            .lock()
            .map_err(|_| ApprovalCommandError::StateUnavailable)? =
            PendingApprovalRoute::Waiting(request.approval_id.clone());
        self.state = ApprovalState::Waiting { request };
        Ok(())
    }

    pub async fn wait_for_resolution(&mut self, cancel: &CancellationToken) -> ApprovalWaitOutcome {
        let request = match &self.state {
            ApprovalState::Waiting { request } => request.clone(),
            ApprovalState::Idle
            | ApprovalState::Resolved { .. }
            | ApprovalState::Cancelled { .. } => return ApprovalWaitOutcome::CommandChannelClosed,
        };

        let (outcome, acknowledgement) = tokio::select! {
            biased;
            _ = cancel.cancelled() => (ApprovalWaitOutcome::Cancelled, None),
            envelope = self.receiver.recv() => match envelope {
                Some(envelope) => {
                    let resolution = envelope.command.resolution;
                    (
                        ApprovalWaitOutcome::Resolved(resolution),
                        Some(envelope.acknowledgement),
                    )
                }
                None => (ApprovalWaitOutcome::CommandChannelClosed, None),
            },
        };

        let Ok(mut pending) = self.pending_approval.lock() else {
            if let Some(acknowledgement) = acknowledgement {
                let _ = acknowledgement.send(Err(ApprovalCommandError::StateUnavailable));
            }
            self.state = ApprovalState::Cancelled { request };
            return ApprovalWaitOutcome::StateUnavailable;
        };
        *pending = PendingApprovalRoute::Idle;
        drop(pending);
        self.state = match &outcome {
            ApprovalWaitOutcome::Resolved(resolution) => ApprovalState::Resolved {
                request,
                resolution: resolution.clone(),
            },
            ApprovalWaitOutcome::Cancelled
            | ApprovalWaitOutcome::CommandChannelClosed
            | ApprovalWaitOutcome::StateUnavailable => ApprovalState::Cancelled { request },
        };
        if let Some(acknowledgement) = acknowledgement {
            let _ = acknowledgement.send(Ok(()));
        }
        outcome
    }

    pub fn state(&self) -> &ApprovalState {
        &self.state
    }

    pub fn into_state(self) -> ApprovalState {
        self.state
    }
}

pub fn turn_command_channel(turn_id: TurnId) -> (TurnCommandHandle, TurnCommandInbox) {
    let (sender, receiver) = mpsc::channel(TURN_COMMAND_CAPACITY);
    let pending_approval = Arc::new(Mutex::new(PendingApprovalRoute::Idle));
    (
        TurnCommandHandle {
            turn_id: turn_id.clone(),
            sender,
            pending_approval: Arc::clone(&pending_approval),
        },
        TurnCommandInbox {
            turn_id,
            receiver,
            pending_approval,
            state: ApprovalState::Idle,
        },
    )
}
