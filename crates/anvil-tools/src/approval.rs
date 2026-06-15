//! 审批策略层:何时需要审批([`ApprovalPolicy`])、谁来审([`ApprovalsReviewer`]),
//! 以及把"问用户"的异步决定回传给 agent loop 的桥([`ApprovalBridge`])。
//!
//! 对齐 codex 的双维度设计:`AskForApproval`(何时审)× `ApprovalsReviewer`(谁审)。
//! 审批的真正决策发生在编排层(agent loop),工具内部不再自行审批。

use std::collections::HashMap;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::{Mutex, oneshot};

/// 一次工具调用的最终审批结论。
#[derive(Debug, Clone)]
pub enum ApprovalDecision {
    /// 允许执行。
    Allow,
    /// 拒绝执行,携带原因回传给模型。
    Deny(String),
}

/// 何时需要对工具调用发起审批。对应 codex 的 `AskForApproval`。
///
/// 当前仅 [`ApprovalPolicy::Untrusted`] 与 [`ApprovalPolicy::Never`] 有真实语义;
/// `OnFailure` / `OnRequest` / `Granular` 依赖沙箱判定,在无沙箱时
/// [`ApprovalPolicy::requires_approval`] 一律保守地视为"需要审批"(等同 `Untrusted`)。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalPolicy {
    /// 不信任环境:每一次工具调用都要审批。
    #[default]
    Untrusted,
    /// 先执行,失败(非零退出等)才审批。无沙箱时降级为需要审批。
    OnFailure,
    /// 越出沙箱边界才审批。无沙箱时降级为需要审批。
    OnRequest,
    /// 细粒度规则。无沙箱时降级为需要审批。
    Granular,
    /// 从不审批,全自动放行。
    Never,
}

impl ApprovalPolicy {
    /// 该工具调用是否需要发起审批。
    ///
    /// 无沙箱时,除 [`ApprovalPolicy::Never`] 外一律返回 `true`(保守:宁可多问)。
    /// 待接入沙箱后,`OnFailure` / `OnRequest` / `Granular` 会在此细化判定。
    pub fn requires_approval(&self, _tool: &str, _input: &Value) -> bool {
        match self {
            ApprovalPolicy::Never => false,
            ApprovalPolicy::Untrusted
            | ApprovalPolicy::OnFailure
            | ApprovalPolicy::OnRequest
            | ApprovalPolicy::Granular => true,
        }
    }
}

/// 需要审批时由谁来审。对应 codex 的 `ApprovalsReviewer`。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalsReviewer {
    /// 真人:经 [`ApprovalBridge`] 异步等待宿主(前端 UI)确认。
    #[default]
    User,
    /// guardian 风格的 LLM 子会话自动审。当前仅占位,编排层会保守拒绝。
    AutoReview,
}

/// 异步回传桥:agent loop 在此注册一个 pending 审批并 `await`,
/// 宿主(前端 / 测试)稍后用相同 id 调 [`ApprovalBridge::resolve`] 回传决定。
///
/// `Clone` 廉价(内部 `Arc`),可同时塞进 `tauri::State` 与 `AgentConfig`。
/// 必须满足 `Send + Sync` 以便 `tauri::State` 共享。
#[derive(Clone)]
pub struct ApprovalBridge {
    pending: Arc<Mutex<HashMap<String, oneshot::Sender<ApprovalDecision>>>>,
}

impl ApprovalBridge {
    pub fn new() -> Self {
        Self {
            pending: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// agent 端:注册一个 pending 请求,返回 receiver 供 `await`。
    /// 若 id 已存在会被覆盖(正常情况下 id 复用模型工具调用 id,唯一)。
    pub async fn register(&self, id: &str) -> oneshot::Receiver<ApprovalDecision> {
        let (tx, rx) = oneshot::channel();
        self.pending.lock().await.insert(id.to_string(), tx);
        rx
    }

    /// 宿主端:回传决定。若无对应 pending(已取消 / id 未知)返回 `Err`。
    pub async fn resolve(&self, id: &str, decision: ApprovalDecision) -> Result<(), String> {
        match self.pending.lock().await.remove(id) {
            Some(tx) => {
                let _ = tx.send(decision);
                Ok(())
            }
            None => Err(format!("no pending approval for id {id}")),
        }
    }
}

impl Default for ApprovalBridge {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for ApprovalBridge {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ApprovalBridge").finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn never_does_not_require_approval() {
        let policy = ApprovalPolicy::Never;
        assert!(!policy.requires_approval("bash", &serde_json::json!({})));
        assert!(!policy.requires_approval("write", &serde_json::json!({})));
    }

    #[test]
    fn untrusted_always_requires_approval() {
        let policy = ApprovalPolicy::Untrusted;
        assert!(policy.requires_approval("bash", &serde_json::json!({})));
        assert!(policy.requires_approval("read", &serde_json::json!({})));
    }

    #[test]
    fn sandbox_pending_modes_conservatively_require_approval() {
        // 无沙箱时,这些模式一律保守降级为"需要审批"。
        for policy in [
            ApprovalPolicy::OnFailure,
            ApprovalPolicy::OnRequest,
            ApprovalPolicy::Granular,
        ] {
            assert!(
                policy.requires_approval("bash", &serde_json::json!({})),
                "{policy:?} should require approval without a sandbox"
            );
        }
    }

    #[test]
    fn defaults_are_safe() {
        // 默认策略 = Untrusted(最严),默认审阅者 = User。
        assert_eq!(ApprovalPolicy::default(), ApprovalPolicy::Untrusted);
        assert_eq!(ApprovalsReviewer::default(), ApprovalsReviewer::User);
    }

    #[tokio::test]
    async fn bridge_resolve_delivers_decision_to_registerer() {
        let bridge = ApprovalBridge::new();
        let rx = bridge.register("call-1").await;
        bridge
            .resolve("call-1", ApprovalDecision::Allow)
            .await
            .expect("pending request exists");

        let decision = rx.await.expect("channel not closed");
        assert!(matches!(decision, ApprovalDecision::Allow));
    }

    #[tokio::test]
    async fn bridge_resolve_unknown_id_errors() {
        let bridge = ApprovalBridge::new();
        let err = bridge
            .resolve("missing", ApprovalDecision::Deny("no".into()))
            .await
            .unwrap_err();
        assert!(err.contains("missing"));
    }

    #[tokio::test]
    async fn bridge_clone_shares_state() {
        // clone 后两份共享同一份 pending:一份 register,另一份 resolve。
        let a = ApprovalBridge::new();
        let b = a.clone();
        let rx = a.register("shared").await;
        b.resolve("shared", ApprovalDecision::Deny("denied".into()))
            .await
            .unwrap();
        let decision = rx.await.unwrap();
        assert!(matches!(decision, ApprovalDecision::Deny(_)));
    }

    #[test]
    fn bridge_is_send_sync() {
        // 编译期断言:tauri::State 要求 Send + Sync。
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<ApprovalBridge>();
    }
}
