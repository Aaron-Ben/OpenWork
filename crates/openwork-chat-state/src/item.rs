use openwork_models::model::Message;
use serde::{Deserialize, Serialize};

/// What a persisted Message *is*, independent of its `role`.
///
/// Several Conversation entries are User-role but are not user requests:
/// Skill bodies materialized at Turn acceptance, and messages a sub-agent sent
/// back to its parent. They must reach the model and the compaction summarizer,
/// yet they are not what the user typed.
///
/// **Do not infer this from `role`.** `role` is the provider-facing transport;
/// the same role carries all three kinds. Code that means "the user's request"
/// must test this enum — see `last_real_user` in the compaction projection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageKind {
    /// A user-visible message, or any assistant / tool message.
    #[default]
    Normal,
    /// Skill body snapshot written before the user-visible request.
    SkillInstruction,
    /// A message a sub-agent delivered to its parent Session.
    AgentMessage,
}

impl MessageKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Normal => "normal",
            Self::SkillInstruction => "skill_instruction",
            Self::AgentMessage => "agent_message",
        }
    }

    /// True when the entry is model-visible context rather than a user request.
    pub fn is_contextual(self) -> bool {
        !matches!(self, Self::Normal)
    }
}

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
    pub kind: MessageKind,
    pub message: Message,
}

impl ConversationItem {
    pub fn real(message: Message) -> Self {
        Self {
            origin: ConversationItemOrigin::Real {
                message_id: None,
                sequence: None,
            },
            kind: MessageKind::Normal,
            message,
        }
    }

    pub fn persisted(message_id: impl Into<String>, sequence: i64, message: Message) -> Self {
        Self::persisted_with_kind(message_id, sequence, MessageKind::Normal, message)
    }

    pub fn persisted_with_kind(
        message_id: impl Into<String>,
        sequence: i64,
        kind: MessageKind,
        message: Message,
    ) -> Self {
        Self {
            origin: ConversationItemOrigin::Real {
                message_id: Some(message_id.into()),
                sequence: Some(sequence),
            },
            kind,
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
            kind: MessageKind::Normal,
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
            kind: MessageKind::Normal,
            message,
        }
    }

    pub fn is_real(&self) -> bool {
        matches!(self.origin, ConversationItemOrigin::Real { .. })
    }

    /// True when this entry is model-visible context rather than something the
    /// user typed. Both Skill bodies and sub-agent messages are User-role.
    pub fn is_contextual(&self) -> bool {
        self.kind.is_contextual()
    }

    pub fn synthetic_reason(&self) -> Option<SyntheticReason> {
        match self.origin {
            ConversationItemOrigin::Synthetic { reason, .. } => Some(reason),
            ConversationItemOrigin::Real { .. } => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use openwork_models::model::{ContentBlock, Role};

    fn user(text: &str) -> Message {
        Message {
            role: Role::User,
            content: vec![ContentBlock::text(text)],
        }
    }

    #[test]
    fn constructors_default_to_normal_kind() {
        assert_eq!(ConversationItem::real(user("hi")).kind, MessageKind::Normal);
        assert_eq!(
            ConversationItem::persisted("msg-1", 1, user("hi")).kind,
            MessageKind::Normal
        );
    }

    #[test]
    fn contextual_kinds_stay_real_but_are_not_user_requests() {
        for kind in [MessageKind::SkillInstruction, MessageKind::AgentMessage] {
            let item = ConversationItem::persisted_with_kind("msg-1", 1, kind, user("body"));
            // They are genuine rows in `messages`; only their meaning differs.
            assert!(item.is_real(), "{kind:?} must stay real");
            assert!(item.is_contextual(), "{kind:?} must be contextual");
        }
    }

    #[test]
    fn kind_serializes_as_snake_case() {
        let encoded = serde_json::to_string(&MessageKind::AgentMessage).expect("serialize");
        assert_eq!(encoded, "\"agent_message\"");
        assert_eq!(MessageKind::AgentMessage.as_str(), "agent_message");
    }
}
