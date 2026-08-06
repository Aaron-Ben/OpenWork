use openwork_chat_state::{
    ConversationCompactionView, ConversationItem, ConversationItemOrigin, SyntheticReason,
};
use openwork_models::model::{Message, Role};

use super::{CompactionError, ConversationCompaction};

const SUMMARY_PREFIX: &str = "The earlier Conversation was compacted into the following continuation summary. Treat it as prior conversation context, preserve its uncertainty, and continue from it:\n\n";

pub(super) fn last_real_user(source: &ConversationCompactionView) -> Option<&ConversationItem> {
    source.items.iter().rev().find(|item| {
        item.message.role == Role::User
            && (item.is_real()
                || item.synthetic_reason() == Some(SyntheticReason::LastUserRequestReplay))
    })
}

pub(super) fn last_user_source(origin: &ConversationItemOrigin) -> (Option<String>, Option<i64>) {
    match origin {
        ConversationItemOrigin::Real {
            message_id,
            sequence,
        } => (message_id.clone(), *sequence),
        ConversationItemOrigin::Synthetic {
            reason: SyntheticReason::LastUserRequestReplay,
            source_message_id,
            source_sequence,
            ..
        } => (source_message_id.clone(), *source_sequence),
        ConversationItemOrigin::Synthetic { .. } => (None, None),
    }
}

pub(crate) fn compacted_items(
    compaction: &ConversationCompaction,
    last_user: Message,
) -> Result<Vec<ConversationItem>, CompactionError> {
    let message_id = compaction
        .last_user_message_id
        .as_deref()
        .ok_or(CompactionError::MissingLastUser)?;
    let message_sequence = compaction
        .last_user_message_sequence
        .ok_or(CompactionError::MissingLastUser)?;
    if last_user.content.is_empty() {
        return Err(CompactionError::MissingLastUser);
    }
    Ok(vec![
        ConversationItem::last_user_replay(&compaction.id, message_id, message_sequence, last_user),
        ConversationItem::synthetic(
            &compaction.id,
            SyntheticReason::CompactionSummary,
            compaction_summary_message(&compaction.summary),
        ),
        ConversationItem::synthetic(
            &compaction.id,
            SyntheticReason::SystemReminder,
            Message::text(Role::User, compaction.runtime_reminder.clone()),
        ),
    ])
}

pub(crate) fn compaction_summary_message(summary: &str) -> Message {
    Message::text(Role::User, format!("{SUMMARY_PREFIX}{}", summary.trim()))
}

#[cfg(test)]
mod tests {
    use openwork_models::model::{ContentBlock, Message, Role};

    use super::compacted_items;
    use crate::session::{
        CompactionRuntimeState, ConversationCompaction, ConversationCompactionKind,
        NewConversationCompaction, SessionId,
    };

    #[test]
    fn last_user_replay_keeps_the_visible_user_message() {
        let compaction = ConversationCompaction::in_memory(
            &SessionId::new("session-1"),
            &NewConversationCompaction {
                kind: ConversationCompactionKind::Manual,
                source_message_count: 1,
                resolved_model_name: "test-model".to_string(),
                summary: "Summary".to_string(),
                runtime_state: CompactionRuntimeState::default(),
                runtime_reminder: "Reminder".to_string(),
                input_tokens: None,
                output_tokens: None,
                trigger_turn_id: None,
                last_user_message_id: Some("message-1".to_string()),
                last_user_message_sequence: Some(1),
            },
        );
        let user = Message::text(Role::User, "Use $commit.");

        let items = compacted_items(&compaction, user).expect("replacement");

        assert_eq!(
            items[0].message.content,
            [ContentBlock::text("Use $commit.")]
        );
    }
}
