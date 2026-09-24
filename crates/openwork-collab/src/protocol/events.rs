use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct InvalidationEvent {
    pub id: String,
    pub kind: InvalidationKind,
    pub subject_id: Option<String>,
    pub revision: Option<i64>,
    pub published_at: i64,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum InvalidationKind {
    RuntimeReady,
    AgentConfig,
    Room,
    Message,
    Board,
    EngineInventory,
    RunnerStatus,
    /// Run 打开或结束、卡片唤醒写入：Agent 的当前状态与卡片的 `agentState` 可能变了。
    AgentActivity,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WakeEvent {
    pub id: String,
    pub agent_id: String,
    /// 触发唤醒的消息或卡片 id。
    pub subject_id: String,
    pub reason: WakeReason,
    pub published_at: i64,
}

/// Agent 被叫醒的原因；Runner 收到任何一种都重新读持久收件箱。
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub enum WakeReason {
    #[serde(rename = "message.new")]
    MessageNew,
    #[serde(rename = "card.wake")]
    CardWake,
}
