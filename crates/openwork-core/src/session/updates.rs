use serde::{Deserialize, Serialize};
use serde_json::Value;

use openwork_models::model::ToolResultArtifact;
use openwork_tools::PermissionMode;

use crate::plan::{PlanStep, TurnPlanSnapshot};

use super::{
    ClientRequestId, PermissionDecision, PermissionRequest, SessionId, ToolCallId, TurnId,
    TurnOutcome,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionPhase {
    Starting,
    RunningModel,
    RunningTools,
    WaitingPermission,
    /// An automatic (threshold or overflow) compaction is summarizing the
    /// conversation inside the active Turn.
    Compacting,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LiveToolCall {
    pub tool_call_id: ToolCallId,
    pub provider_call_id: String,
    pub name: String,
    pub input: Value,
    pub status: String,
    pub output: Option<String>,
    pub is_error: Option<bool>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub artifacts: Vec<ToolResultArtifact>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum ToolProgressUpdate {
    Stdout { chunk: String },
    Stderr { chunk: String },
    Message { message: String },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "state",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum SessionRuntimeSnapshot {
    Idle,
    Running {
        turn_id: TurnId,
        client_request_id: ClientRequestId,
        phase: SessionPhase,
        draft_text: String,
        draft_reasoning: String,
        tool_calls: Vec<LiveToolCall>,
        pending_permission: Option<Box<PermissionRequest>>,
        plan: Option<TurnPlanSnapshot>,
    },
    /// Turn 已经结束但仍是同进程可见的最后状态。
    ///
    /// 这里也带计划：否则 Turn 刚结束时重连会丢掉卡片，而它明明还在 `turn_plans` 里。
    Terminal {
        turn_id: TurnId,
        client_request_id: ClientRequestId,
        outcome: TurnOutcome,
        plan: Option<TurnPlanSnapshot>,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionSnapshot {
    pub version: u16,
    pub session_id: SessionId,
    pub last_update_sequence: u64,
    pub permission_mode: PermissionMode,
    pub runtime: SessionRuntimeSnapshot,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum SessionUpdate {
    TurnStarted {
        client_request_id: ClientRequestId,
    },
    PhaseChanged {
        phase: SessionPhase,
    },
    TextDelta {
        delta: String,
    },
    ReasoningDelta {
        delta: String,
    },
    DraftCleared,
    ToolCallStarted {
        tool_call: LiveToolCall,
    },
    ToolCallProgress {
        tool_call_id: ToolCallId,
        progress: ToolProgressUpdate,
    },
    ToolCallFinished {
        tool_call_id: ToolCallId,
        provider_call_id: String,
        tool_name: String,
        status: String,
        output: String,
        is_error: bool,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        artifacts: Vec<ToolResultArtifact>,
    },
    PermissionRequested {
        request: PermissionRequest,
    },
    PermissionResolved {
        tool_call_id: ToolCallId,
        decision: PermissionDecision,
        permission_mode: PermissionMode,
    },
    /// 当前 Turn 的计划已经变成这个完整快照。
    ///
    /// 携带全量而不是增量：前端不需要重放 patch，漏掉一条也能从 Snapshot 或持久层恢复。
    /// 只在持久化成功之后发送——事件是可丢的投影，不是业务真相。
    ///
    /// `updated_at` 带 `+08:00`，与历史计划的字段同形，好让前端对实时与历史用同一套渲染。
    /// 它也让 Actor 能无损地把事件折进 Snapshot——少了它，快照就得另找时间来源。
    PlanUpdated {
        explanation: Option<String>,
        plan: Vec<PlanStep>,
        updated_at: String,
    },
    TurnFinished {
        outcome: TurnOutcome,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionUpdateEnvelope {
    pub version: u16,
    pub session_id: SessionId,
    pub turn_id: TurnId,
    pub sequence: u64,
    pub occurred_at_ms: u64,
    pub update: SessionUpdate,
}
