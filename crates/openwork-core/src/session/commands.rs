use std::time::Duration;

use openwork_models::model::{ModelCallOptions, ModelCapabilities};
use openwork_models::provider::{
    DEFAULT_REQUEST_MAX_RETRIES, DEFAULT_STREAM_IDLE_TIMEOUT_MS, ProviderSettings,
};
use openwork_tools::ToolResultStatus;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use super::approval::ApprovalCard;
use super::{ClientRequestId, SessionId, ToolCallId, TurnId};

/// 一个 Session 使用的模型：Provider 配置 id、模型名、能力与调用设置。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedModel {
    /// 模型引用 `<providerId>/<modelId>`。
    pub model_id: Option<String>,
    pub provider_id: String,
    pub model_name: String,
    pub capabilities: ModelCapabilities,
    /// 一次 Model Call 的传输尝试次数，包含第一次。取自 Provider 配置的 `requestMaxRetries`。
    pub max_transport_attempts: usize,
    /// 两个流事件之间的最长间隔。取自 Provider 配置的 `streamIdleTimeoutMs`。
    pub stream_idle_timeout_ms: u64,
    /// 发送的 `reasoning.effort`：Session 选的档位，或目录中的默认档位。`None` 时不发送。
    pub reasoning_effort: Option<String>,
}

impl ResolvedModel {
    pub fn new(
        model_id: Option<impl Into<String>>,
        provider_id: impl Into<String>,
        model_name: impl Into<String>,
        capabilities: ModelCapabilities,
    ) -> Self {
        Self {
            model_id: model_id.map(Into::into),
            provider_id: provider_id.into(),
            model_name: model_name.into(),
            capabilities,
            max_transport_attempts: DEFAULT_REQUEST_MAX_RETRIES as usize + 1,
            stream_idle_timeout_ms: DEFAULT_STREAM_IDLE_TIMEOUT_MS,
            reasoning_effort: None,
        }
    }

    pub fn with_reasoning_effort(mut self, effort: Option<String>) -> Self {
        self.reasoning_effort = effort;
        self
    }

    pub fn with_provider_settings(mut self, settings: &ProviderSettings) -> Self {
        self.max_transport_attempts = settings.request_max_retries() as usize + 1;
        self.stream_idle_timeout_ms = settings.stream_idle_timeout_ms();
        self
    }

    /// 一次 Model Call 的参数。Trace 与重试层读同一份次数。
    pub fn call_options(&self, model_attempt_id: impl Into<String>) -> ModelCallOptions {
        ModelCallOptions::new(model_attempt_id)
            .with_max_transport_attempts(self.max_transport_attempts)
            .with_idle_timeout(Duration::from_millis(self.stream_idle_timeout_ms))
    }
}

/// 卡片上只有两个按钮（permissions.md §12.2）：没有"本会话允许"，卡片也不切换模式。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PermissionDecision {
    AllowOnce,
    Deny,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PermissionRequest {
    pub session_id: SessionId,
    pub turn_id: TurnId,
    pub tool_call_id: ToolCallId,
    pub provider_call_id: String,
    pub tool_name: String,
    pub card: ApprovalCard,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TurnAccepted {
    pub turn_id: TurnId,
    pub client_request_id: ClientRequestId,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "status",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum TurnOutcome {
    Completed { final_text: String },
    Failed { code: String, message: String },
    Cancelled,
}

impl TurnOutcome {
    pub fn terminal_tool_status(&self) -> ToolResultStatus {
        match self {
            Self::Completed { .. } => ToolResultStatus::Succeeded,
            Self::Failed { .. } => ToolResultStatus::Failed,
            Self::Cancelled => ToolResultStatus::Cancelled,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum SessionError {
    #[error("session actor stopped")]
    ActorStopped,
    #[error("session already has an active turn: {0}")]
    Busy(TurnId),
    #[error("turn is not active: {0}")]
    TurnNotActive(TurnId),
    #[error("permission request is not pending for tool call: {0}")]
    PermissionNotPending(ToolCallId),
    #[error("turn input must not be empty")]
    EmptyInput,
    #[error(
        "session Conversation checkpoint changed but the in-memory projection could not be installed; reload the Session"
    )]
    ReloadRequired,
}
