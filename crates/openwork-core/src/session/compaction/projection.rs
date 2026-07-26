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
