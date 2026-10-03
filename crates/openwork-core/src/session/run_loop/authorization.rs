//! 一次工具调用执行前的授权（permissions.md §1、§12）。
//!
//! 事实来自工具（[`openwork_tools::FinalizedToolset::prepare`]），判定在
//! [`super::super::approval`]，这里负责和用户往返：发出卡片、等待决定、记录 Trace。

use std::sync::Arc;
use std::time::Instant;

use openwork_models::model::ToolCallBlock;
use openwork_sandbox::{SandboxMode, SandboxPolicy};
use openwork_tools::{ToolErrorCode, ToolInvocation, ToolResult, ToolValidationError};
use tokio::sync::oneshot;

use super::super::approval::{Gate, gate};
use super::super::{PermissionDecision, PermissionRequest, ToolCallId, ToolCallTraceGuard};
use super::{RunnerEvent, TurnRunError, TurnRunner, elapsed_millis};

/// 授权的结果。
pub(super) enum Authorized {
    /// 在这个策略下执行；Trace 继续由调用方记录。装箱是因为 guard 很大，而另一个变体没有数据。
    Run {
        policy: SandboxPolicy,
        tool_trace: Box<ToolCallTraceGuard>,
    },
    /// 已经把结果写回会话（规则拒绝、输入无效），不执行。
    Handled,
}

impl TurnRunner {
    pub(super) async fn authorize_tool_call(
        &mut self,
        call: &ToolCallBlock,
        tool_call_id: &ToolCallId,
        tool_name: &str,
        invocation: &ToolInvocation,
        mode: SandboxMode,
        mut tool_trace: ToolCallTraceGuard,
    ) -> Result<Authorized, TurnRunError> {
        let session_policy = self.request.sandbox.policy(mode, Vec::new());
        let tools = Arc::clone(self.request.tools.registered());
        let prepared = match tools.prepare(invocation, &session_policy).await {
            Ok(prepared) => prepared,
            Err(error) => {
                let code = match error {
                    ToolValidationError::UnknownTool(_) => ToolErrorCode::ToolNotFound,
                    ToolValidationError::InvalidInput(_) => ToolErrorCode::InvalidArguments,
                };
                let result = ToolResult::failed(code, error.to_string(), false);
                self.append_tool_result(call, tool_call_id.clone(), result, tool_trace)
                    .await?;
                return Ok(Authorized::Handled);
            }
        };
        if let Some(escalation) = &prepared.escalation {
            tool_trace.record_escalation(&escalation.grants, &escalation.justification);
        }
        if let Some(danger) = &prepared.danger {
            tool_trace.record_danger(danger.key);
        }

        match gate(
            &prepared,
            &session_policy,
            self.request.approval,
            self.last_sandbox_denial.as_deref(),
        ) {
            Gate::Run => {
                tool_trace.record_permission_decision("allow", "sandbox");
                Ok(Authorized::Run {
                    policy: session_policy,
                    tool_trace: Box::new(tool_trace),
                })
            }
            Gate::Deny { text, source } => {
                tool_trace.record_permission_decision("deny", source.as_str());
                self.append_tool_result(
                    call,
                    tool_call_id.clone(),
                    ToolResult::denied(text),
                    tool_trace,
                )
                .await?;
                Ok(Authorized::Handled)
            }
            Gate::Ask { card, grants } => {
                let request = PermissionRequest {
                    session_id: self.request.session_id.clone(),
                    turn_id: self.request.turn_id.clone(),
                    tool_call_id: tool_call_id.clone(),
                    provider_call_id: call.id.clone(),
                    tool_name: tool_name.to_string(),
                    card,
                };
                match self.ask_user(request, &mut tool_trace).await {
                    Some(PermissionDecision::AllowOnce) => {
                        tool_trace.record_permission_decision("allow", "user");
                        Ok(Authorized::Run {
                            policy: self.request.sandbox.policy(mode, grants),
                            tool_trace: Box::new(tool_trace),
                        })
                    }
                    Some(PermissionDecision::Deny) => {
                        // 用户明确表态，Turn 停下（permissions.md §12.3）。
                        tool_trace.record_permission_decision("deny", "user");
                        let result = ToolResult::denied(USER_DENIED);
                        self.append_tool_result(call, tool_call_id.clone(), result, tool_trace)
                            .await?;
                        Err(TurnRunError::PermissionDenied(USER_DENIED.to_string()))
                    }
                    None => {
                        tool_trace.record_permission_decision("cancelled", "system");
                        let cancelled = self.request.cancel.is_cancelled();
                        let result = if cancelled {
                            ToolResult::cancelled("turn cancelled while waiting for permission")
                        } else {
                            ToolResult::outcome_unknown("permission responder stopped")
                        };
                        self.append_tool_result(call, tool_call_id.clone(), result, tool_trace)
                            .await?;
                        Err(if cancelled {
                            TurnRunError::Cancelled
                        } else {
                            TurnRunError::ActorStopped
                        })
                    }
                }
            }
        }
    }

    /// 发出卡片并等待决定。Turn 被取消、或会话 actor 已经停止时返回 `None`。
    async fn ask_user(
        &self,
        request: PermissionRequest,
        tool_trace: &mut ToolCallTraceGuard,
    ) -> Option<PermissionDecision> {
        let (respond_to, decision) = oneshot::channel();
        self.request
            .events
            .send(RunnerEvent::PermissionRequested {
                request,
                respond_to,
            })
            .await
            .ok()?;
        let wait_started = Instant::now();
        let decision = tokio::select! {
            _ = self.request.cancel.cancelled() => None,
            result = decision => result.ok(),
        };
        tool_trace.record_permission_wait_ms(elapsed_millis(wait_started));
        decision
    }
}

const USER_DENIED: &str = "user denied tool permission";

/// 被沙箱拒绝的结果里最能说明原因的一行，放到同一 Turn 下一张越界卡片上（permissions.md §12.1）。
pub(super) fn denial_line(result: &ToolResult) -> Option<String> {
    let text = result.text_content();
    text.lines()
        .find(|line| {
            line.to_ascii_lowercase()
                .contains("operation not permitted")
        })
        .or_else(|| text.lines().find(|line| line.starts_with("[sandbox:")))
        .map(|line| line.trim().to_string())
}
