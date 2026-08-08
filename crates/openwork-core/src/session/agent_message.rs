use std::collections::VecDeque;
use std::sync::Arc;
use std::time::Duration;

use openwork_models::model::{Message, Role};
use serde::{Deserialize, Serialize};
use tokio::sync::{Mutex, Notify};

use super::SessionId;
use crate::agent::AgentControl;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentMessageKind {
    FinalAnswer,
    Interrupted,
    Failed,
}

impl AgentMessageKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::FinalAnswer => "final_answer",
            Self::Interrupted => "interrupted",
            Self::Failed => "failed",
        }
    }
}

#[derive(Clone)]
pub struct ParentLink {
    pub parent_session_id: SessionId,
    pub task_name: String,
    pub agent_control: AgentControl,
}

impl std::fmt::Debug for ParentLink {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ParentLink")
            .field("parent_session_id", &self.parent_session_id)
            .field("task_name", &self.task_name)
            .finish_non_exhaustive()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct AgentMessage {
    pub task_name: String,
    pub kind: AgentMessageKind,
    pub body: String,
}

impl AgentMessage {
    pub fn into_model_message(self) -> Message {
        Message::text(
            Role::User,
            format!(
                "<agent_message>\n<task>{}</task>\n<kind>{}</kind>\n<body>\n{}\n</body>\n</agent_message>",
                self.task_name,
                self.kind.as_str(),
                self.body
            ),
        )
    }
}

/// In-memory queue of sub-agent results that have not entered the parent
/// Conversation yet. Delivery never starts a Turn; only a running Turn drains
/// the queue before its next Model Call.
#[derive(Debug, Clone, Default)]
pub(super) struct AgentMailbox {
    inner: Arc<AgentMailboxInner>,
}

#[derive(Debug, Default)]
struct AgentMailboxInner {
    pending: Mutex<VecDeque<AgentMessage>>,
    delivered: Notify,
}

impl AgentMailbox {
    pub async fn push(&self, message: AgentMessage) {
        self.inner.pending.lock().await.push_back(message);
        self.inner.delivered.notify_one();
    }

    pub async fn front(&self) -> Option<AgentMessage> {
        self.inner.pending.lock().await.front().cloned()
    }

    pub async fn pop_front(&self) {
        self.inner.pending.lock().await.pop_front();
    }

    /// 等到至少一条未消费的回传可用；超时返回 `false`，且不消费消息。
    pub async fn wait_for_delivery(&self, timeout: Duration) -> bool {
        tokio::time::timeout(timeout, async {
            loop {
                // 先创建通知 future，再检查队列。消息若恰好在两步之间到达，Notify 会保留
                // permit；反过来先检查则可能错过这次唤醒并一直等到超时。
                let delivered = self.inner.delivered.notified();
                if !self.inner.pending.lock().await.is_empty() {
                    return;
                }
                // drain 可能已经消费了旧消息但留下一个 permit；醒来后必须重新检查队列，
                // 不能把旧信号误报成一次新投递。
                delivered.await;
            }
        })
        .await
        .is_ok()
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use openwork_models::model::ContentBlock;

    use super::*;

    #[test]
    fn final_answer_uses_the_agent_message_envelope_and_user_role() {
        let message = AgentMessage {
            task_name: "find_auth_flow".to_string(),
            kind: AgentMessageKind::FinalAnswer,
            body: "Found it.".to_string(),
        }
        .into_model_message();

        assert_eq!(message.role, Role::User);
        assert_eq!(
            message.content,
            [ContentBlock::text(
                "<agent_message>\n<task>find_auth_flow</task>\n<kind>final_answer</kind>\n<body>\nFound it.\n</body>\n</agent_message>"
            )]
        );
    }

    #[tokio::test]
    async fn waiting_returns_immediately_when_delivery_is_already_pending() {
        let mailbox = AgentMailbox::default();
        mailbox
            .push(AgentMessage {
                task_name: "ready".to_string(),
                kind: AgentMessageKind::FinalAnswer,
                body: "done".to_string(),
            })
            .await;

        let delivered = tokio::time::timeout(
            Duration::from_millis(50),
            mailbox.wait_for_delivery(Duration::from_secs(1)),
        )
        .await
        .expect("pre-existing delivery must not wait");

        assert!(delivered);
    }

    #[tokio::test]
    async fn waiting_observes_delivery_that_races_with_subscription() {
        let mailbox = AgentMailbox::default();
        let sender = mailbox.clone();
        tokio::spawn(async move {
            tokio::task::yield_now().await;
            sender
                .push(AgentMessage {
                    task_name: "racing".to_string(),
                    kind: AgentMessageKind::FinalAnswer,
                    body: "done".to_string(),
                })
                .await;
        });

        assert!(mailbox.wait_for_delivery(Duration::from_secs(1)).await);
    }

    #[tokio::test]
    async fn waiting_reports_timeout_without_consuming_future_deliveries() {
        let mailbox = AgentMailbox::default();

        assert!(!mailbox.wait_for_delivery(Duration::from_millis(1)).await);
        mailbox
            .push(AgentMessage {
                task_name: "later".to_string(),
                kind: AgentMessageKind::FinalAnswer,
                body: "done".to_string(),
            })
            .await;
        assert!(mailbox.wait_for_delivery(Duration::from_secs(1)).await);
    }

    #[tokio::test]
    async fn a_signal_for_an_already_drained_message_is_not_a_new_delivery() {
        let mailbox = AgentMailbox::default();
        mailbox
            .push(AgentMessage {
                task_name: "drained".to_string(),
                kind: AgentMessageKind::FinalAnswer,
                body: "done".to_string(),
            })
            .await;
        mailbox.pop_front().await;

        assert!(!mailbox.wait_for_delivery(Duration::from_millis(1)).await);
    }
}
