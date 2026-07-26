use openwork_models::model::Message;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyntheticReason {
    LastUserRequestReplay,
    CompactionSummary,
    SystemReminder,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConversationItemOrigin {
    Real {
        message_id: Option<String>,
        sequence: Option<i64>,
    },
    Synthetic {
        compaction_id: String,
        reason: SyntheticReason,
        source_message_id: Option<String>,
        source_sequence: Option<i64>,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct ConversationItem {
    pub origin: ConversationItemOrigin,
    pub message: Message,
}

impl ConversationItem {
    pub fn real(message: Message) -> Self {
        Self {
            origin: ConversationItemOrigin::Real {
                message_id: None,
                sequence: None,
            },
            message,
        }
    }

    pub fn persisted(message_id: impl Into<String>, sequence: i64, message: Message) -> Self {
        Self {
            origin: ConversationItemOrigin::Real {
                message_id: Some(message_id.into()),
                sequence: Some(sequence),
            },
            message,
        }
    }

    pub fn synthetic(
        compaction_id: impl Into<String>,
        reason: SyntheticReason,
        message: Message,
    ) -> Self {
        Self {
            origin: ConversationItemOrigin::Synthetic {
                compaction_id: compaction_id.into(),
                reason,
                source_message_id: None,
                source_sequence: None,
            },
            message,
        }
    }

    pub fn last_user_replay(
        compaction_id: impl Into<String>,
        source_message_id: impl Into<String>,
        source_sequence: i64,
        message: Message,
    ) -> Self {
        Self {
            origin: ConversationItemOrigin::Synthetic {
                compaction_id: compaction_id.into(),
                reason: SyntheticReason::LastUserRequestReplay,
                source_message_id: Some(source_message_id.into()),
                source_sequence: Some(source_sequence),
            },
            message,
        }
    }

    pub fn is_real(&self) -> bool {
        matches!(self.origin, ConversationItemOrigin::Real { .. })
    }

    pub fn synthetic_reason(&self) -> Option<SyntheticReason> {
        match self.origin {
            ConversationItemOrigin::Synthetic { reason, .. } => Some(reason),
            ConversationItemOrigin::Real { .. } => None,
        }
    }
}
