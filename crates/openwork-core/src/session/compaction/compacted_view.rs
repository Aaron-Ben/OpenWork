//! 压缩之后这段 Conversation 由哪些 item 构成。
//!
//! 这里决定的是**成员构成**：摘要替换掉哪一段、边界之后保留哪些原始消息、
//! 重放哪一条真实用户请求。它不裁剪任何内容——按模型能力裁剪属于
//! `context/projection.rs`，两者没有交叠。

use openwork_chat_state::{
    ConversationContextView, ConversationItem, ConversationItemOrigin, SyntheticReason,
};
use openwork_models::model::{Message, Role};

use super::{CompactionError, ConversationCompaction};

const SUMMARY_PREFIX: &str = "The earlier Conversation was compacted into the following continuation summary. Treat it as prior conversation context, preserve its uncertainty, and continue from it:\n\n";

/// Finds the last genuine user *request* to replay after a compaction boundary.
///
/// User role alone is not enough. Skill bodies and sub-agent messages are also
/// persisted with `Role::User` so the model and the summarizer can see them, but
/// neither is something the user asked for. Skill bodies happen to be written
/// *before* the visible request, so ordering used to hide the problem; sub-agent
/// messages arrive mid-Turn, i.e. *after* it, and would otherwise be picked here
/// and replace the real request in every compacted projection.
///
/// **Test `kind`, never ordering.**
pub(super) fn last_real_user(source: &ConversationContextView) -> Option<&ConversationItem> {
    source.items.iter().rev().find(|item| {
        item.message.role == Role::User
            && !item.is_contextual()
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

    use openwork_chat_state::{ConversationContextView, ConversationItem, MessageKind};

    use super::{compacted_items, last_real_user};
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

    fn view(items: Vec<ConversationItem>) -> ConversationContextView {
        ConversationContextView { items }
    }

    fn user_item(sequence: i64, kind: MessageKind, text: &str) -> ConversationItem {
        ConversationItem::persisted_with_kind(
            format!("message-{sequence}"),
            sequence,
            kind,
            Message::text(Role::User, text),
        )
    }

    #[test]
    fn an_agent_message_never_displaces_the_real_user_request() {
        // The ordering this test encodes is the dangerous one: a sub-agent
        // answer lands *after* the user's request, so anything that picks "the
        // last User-role item" would replay the sub-agent's text as though the
        // user had asked for it.
        let source = view(vec![
            user_item(1, MessageKind::Normal, "Where is auth handled?"),
            ConversationItem::persisted(
                "message-2",
                2,
                Message::text(Role::Assistant, "Let me look."),
            ),
            user_item(
                3,
                MessageKind::AgentMessage,
                "<agent_message><task>find_auth</task>…</agent_message>",
            ),
        ]);

        let last_user = last_real_user(&source).expect("a user request must be found");
        assert_eq!(
            last_user.message.content,
            [ContentBlock::text("Where is auth handled?")]
        );
    }

    #[test]
    fn a_skill_instruction_is_not_the_user_request_either() {
        // Skill bodies are written *before* the visible request, so ordering
        // alone used to hide the problem. Pin the behaviour to `kind` so a
        // future reordering cannot resurrect it.
        let source = view(vec![
            user_item(1, MessageKind::SkillInstruction, "<skill>…</skill>"),
            user_item(2, MessageKind::Normal, "Use $commit."),
        ]);
        let last_user = last_real_user(&source).expect("a user request must be found");
        assert_eq!(
            last_user.message.content,
            [ContentBlock::text("Use $commit.")]
        );
    }

    #[test]
    fn a_conversation_with_only_contextual_user_items_has_no_user_request() {
        let source = view(vec![
            user_item(1, MessageKind::SkillInstruction, "<skill>…</skill>"),
            user_item(
                2,
                MessageKind::AgentMessage,
                "<agent_message>…</agent_message>",
            ),
        ]);
        assert!(
            last_real_user(&source).is_none(),
            "contextual entries must never stand in for a user request"
        );
    }

    #[tokio::test]
    async fn a_live_agent_message_never_displaces_the_real_user_request() {
        let chat = openwork_chat_state::ChatStateHandle::spawn(Vec::new()).expect("chat state");
        chat.append_user(vec![ContentBlock::text("Where is auth handled?")])
            .await
            .expect("user request");
        chat.append_user_with_kind(
            vec![ContentBlock::text(
                "<agent_message><task>find_auth</task>…</agent_message>",
            )],
            MessageKind::AgentMessage,
        )
        .await
        .expect("agent message");

        let source = chat.context_view().await.expect("context view");
        let last_user = last_real_user(&source).expect("a user request must be found");
        assert_eq!(
            last_user.message.content,
            [ContentBlock::text("Where is auth handled?")]
        );
    }
}
