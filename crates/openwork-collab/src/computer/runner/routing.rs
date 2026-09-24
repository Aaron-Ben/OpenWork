//! 点名路由在 Computer 一侧的一步（collaboration.md §8.2）：Server 给出路由题时，用本 Agent 的
//! triage 模型回答，再带着答案取回最终的 triage payload。

use tokio_util::sync::CancellationToken;

use crate::protocol::{ResponseMode, TriagePayload};

use super::{
    super::{
        client::RuntimeClientError,
        engine::{ClassifyRequest, EngineUsage},
        triage::parse_route,
    },
    AgentRunner,
};

/// 回答过路由题（或不需要回答）之后的 triage payload。
pub(super) struct RoutedPayload {
    pub(super) payload: TriagePayload,
    /// 回答过路由题时的答案，随最终结论一起上报。
    pub(super) mode: Option<ResponseMode>,
    /// 回答路由题的用量，计入本次 triage。
    pub(super) usage: EngineUsage,
}

impl AgentRunner {
    /// 取本 Run 的 triage payload；其中有路由题时先回答再取一次。模型出错、超时或答案
    /// 无法解析都按“给全员”处理：漏掉该回答的人不会留下痕迹，多跑一次只多花 token。
    pub(super) async fn routed_triage_payload(
        &mut self,
        run_id: &str,
        cancellation: &CancellationToken,
    ) -> Result<RoutedPayload, RuntimeClientError> {
        let payload = self.client.triage_payload(run_id, None).await?;
        let Some(routing) = payload.routing.clone() else {
            return Ok(RoutedPayload {
                payload,
                mode: None,
                usage: EngineUsage::default(),
            });
        };
        let result = async {
            let _permit = self.resources.triage_permit(cancellation).await?;
            self.resources.gate(cancellation).await?;
            self.engine
                .adapter
                .classify(ClassifyRequest {
                    cwd: self.home.work_root.clone(),
                    config_root: self.home.config_root.clone(),
                    confinement: self.home.confinement.clone(),
                    prompt: format!("{}\n\n{}", routing.instructions, routing.input),
                    model: Some(payload.model.clone()),
                    environment: self.home.environment.clone(),
                    cancellation: cancellation.clone(),
                })
                .await
        }
        .await;
        self.resources.observe_result(&result).await;
        let (mode, usage) = match result {
            Ok(result) => (parse_route(&result.text), result.usage),
            Err(error) => {
                tracing::warn!(
                    agent_id = self.assignment.id,
                    %error,
                    "routing question failed; engaging as if addressed to the room"
                );
                (ResponseMode::Each, EngineUsage::default())
            }
        };
        let payload = self.client.triage_payload(run_id, Some(mode)).await?;
        Ok(RoutedPayload {
            payload,
            mode: Some(mode),
            usage,
        })
    }
}
