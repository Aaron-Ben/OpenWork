use std::collections::HashMap;
use std::sync::Mutex;

use openwork_core::{
    ApprovalCommandError, TurnCommandHandle, TurnCommandInbox, turn_command_channel,
};
use openwork_protocol::{approval::ResolveApproval, domain::TurnId};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum TurnSupervisorError {
    #[error("turn already registered: {0}")]
    DuplicateTurn(TurnId),
    #[error("turn not found: {0}")]
    TurnNotFound(TurnId),
    #[error("turn supervisor lock poisoned")]
    LockPoisoned,
    #[error(transparent)]
    Approval(#[from] ApprovalCommandError),
}

/// Application-level routing table. Approval state remains owned by the Core inbox.
#[derive(Default)]
pub struct TurnSupervisor {
    active: Mutex<HashMap<TurnId, TurnCommandHandle>>,
}

impl TurnSupervisor {
    pub fn contains(&self, turn_id: &TurnId) -> Result<bool, TurnSupervisorError> {
        Ok(self
            .active
            .lock()
            .map_err(|_| TurnSupervisorError::LockPoisoned)?
            .contains_key(turn_id))
    }

    pub fn register(&self, turn_id: TurnId) -> Result<TurnCommandInbox, TurnSupervisorError> {
        let mut active = self
            .active
            .lock()
            .map_err(|_| TurnSupervisorError::LockPoisoned)?;
        if active.contains_key(&turn_id) {
            return Err(TurnSupervisorError::DuplicateTurn(turn_id));
        }

        let (handle, inbox) = turn_command_channel(turn_id.clone());
        active.insert(turn_id, handle);
        Ok(inbox)
    }

    pub async fn resolve(&self, command: ResolveApproval) -> Result<(), TurnSupervisorError> {
        let handle = self
            .active
            .lock()
            .map_err(|_| TurnSupervisorError::LockPoisoned)?
            .get(&command.turn_id)
            .cloned()
            .ok_or_else(|| TurnSupervisorError::TurnNotFound(command.turn_id.clone()))?;
        handle.resolve(command).await?;
        Ok(())
    }

    pub fn remove(&self, turn_id: &TurnId) -> Result<bool, TurnSupervisorError> {
        Ok(self
            .active
            .lock()
            .map_err(|_| TurnSupervisorError::LockPoisoned)?
            .remove(turn_id)
            .is_some())
    }
}
