//! Runner 用本 Agent 的 triage 模型回答 Server 给出的 triage 题（collaboration.md §8.3）。模型失败时
//! 照 Cumora daemon 的本地 triage 处理：限流、超时与取消退避重试；输出无法解析或其他 Engine 错误
//! fail closed。走到模型这一步的批次只含 Agent 消息，漏回一条的代价很小。

use tokio_util::sync::CancellationToken;

use crate::protocol::{TriagePayload, TriageVerdict};

use super::{
    super::{
        engine::{ClassifyRequest, EngineError, EngineUsage},
        triage::parse_triage,
    },
    AgentRunner,
};

/// Characters of the failure detail kept in a fail-closed reason (Cumora `errText.slice(0, 120)`).
const FAILURE_DETAIL_MAX_CHARS: usize = 120;

/// triage 模型这一步的结果。
pub(super) enum Classified {
    Verdict {
        verdict: TriageVerdict,
        usage: EngineUsage,
        model: String,
    },
    /// Run 以失败或中断结束，delivery 保留，稍后重试。
    Retry {
        message: String,
        interrupted: bool,
        rate_limited: bool,
    },
}

/// triage 模型失败的原因。
enum Failure<'a> {
    Engine(&'a EngineError),
    Unparseable,
}

impl AgentRunner {
    /// `payload` 带 triage 题（没有现成结论）时调用。
    pub(super) async fn classify_triage(
        &mut self,
        payload: &TriagePayload,
        cancellation: &CancellationToken,
    ) -> Classified {
        let prompt = format!(
            "{}\n\n{}",
            payload.instructions.as_deref().unwrap_or_default(),
            payload.input.as_deref().unwrap_or_default()
        );
        let result = async {
            let _permit = self.resources.triage_permit(cancellation).await?;
            self.resources.gate(cancellation).await?;
            self.engine
                .adapter
                .classify(ClassifyRequest {
                    cwd: self.home.work_root.clone(),
                    config_root: self.home.config_root.clone(),
                    confinement: self.home.confinement.clone(),
                    prompt,
                    model: Some(payload.model.clone()),
                    environment: self.home.environment.clone(),
                    cancellation: cancellation.clone(),
                })
                .await
        }
        .await;
        self.resources.observe_result(&result).await;
        match result {
            Ok(result) => match parse_triage(&result.text) {
                Ok(verdict) => Classified::Verdict {
                    verdict,
                    usage: result.usage,
                    model: result.model.unwrap_or_else(|| payload.model.clone()),
                },
                Err(_) => handle_failure(&Failure::Unparseable, &payload.model),
            },
            Err(error) => handle_failure(&Failure::Engine(&error), &payload.model),
        }
    }
}

/// Cumora daemon：限流与超时退避（fail open 会让主模型在同一份额度上继续失败），取消记为中断；
/// 其他失败 fail closed。
fn handle_failure(failure: &Failure<'_>, model: &str) -> Classified {
    let detail = match failure {
        Failure::Engine(EngineError::Cancelled) => {
            return Classified::Retry {
                message: EngineError::Cancelled.to_string(),
                interrupted: true,
                rate_limited: false,
            };
        }
        Failure::Engine(
            error @ (EngineError::RateLimited { .. } | EngineError::Timeout { .. }),
        ) => {
            return Classified::Retry {
                message: error.to_string(),
                interrupted: false,
                rate_limited: error.is_rate_limited(),
            };
        }
        Failure::Engine(error) => {
            let detail = error.to_string();
            format!(
                "local triage failed ({}); fail closed",
                detail
                    .chars()
                    .take(FAILURE_DETAIL_MAX_CHARS)
                    .collect::<String>()
            )
        }
        Failure::Unparseable => "local triage produced no usable verdict; fail closed".to_string(),
    };
    Classified::Verdict {
        verdict: TriageVerdict {
            actionable: false,
            reason: detail,
            prompt_note: String::new(),
            source: "fail_closed".to_string(),
        },
        usage: EngineUsage::default(),
        model: model.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::{Classified, Failure, handle_failure};
    use crate::computer::engine::EngineError;

    fn retry(classified: Classified) -> (bool, bool) {
        match classified {
            Classified::Retry {
                interrupted,
                rate_limited,
                ..
            } => (interrupted, rate_limited),
            Classified::Verdict { verdict, .. } => panic!("expected a retry, got {verdict:?}"),
        }
    }

    fn fail_closed(classified: Classified) -> (bool, String, String) {
        match classified {
            Classified::Verdict { verdict, .. } => {
                (verdict.actionable, verdict.source, verdict.reason)
            }
            Classified::Retry { message, .. } => {
                panic!("expected fail closed, got retry {message}")
            }
        }
    }

    /// collaboration.md §8.3、§16 #22：限流与超时退避、取消记为中断；无法解析与其他 Engine 错误
    /// fail closed（Cumora daemon 的本地 triage）。
    #[test]
    fn acc_22_triage_failures_back_off_or_fail_closed_like_cumora() {
        let rate_limited = EngineError::RateLimited {
            detail: "429".to_string(),
            retry_after: None,
        };
        assert_eq!(
            retry(handle_failure(
                &Failure::Engine(&rate_limited),
                "local/triage"
            )),
            (false, true)
        );
        let timeout = EngineError::Timeout {
            operation: "classify",
        };
        assert_eq!(
            retry(handle_failure(&Failure::Engine(&timeout), "local/triage")),
            (false, false)
        );
        assert_eq!(
            retry(handle_failure(
                &Failure::Engine(&EngineError::Cancelled),
                "local/triage"
            )),
            (true, false)
        );

        assert_eq!(
            fail_closed(handle_failure(&Failure::Unparseable, "local/triage")),
            (
                false,
                "fail_closed".to_string(),
                "local triage produced no usable verdict; fail closed".to_string()
            )
        );
        let reported = EngineError::Reported {
            detail: "x".repeat(300),
        };
        let (actionable, source, reason) =
            fail_closed(handle_failure(&Failure::Engine(&reported), "local/triage"));
        assert_eq!((actionable, source.as_str()), (false, "fail_closed"));
        assert_eq!(
            reason,
            format!(
                "local triage failed (Engine reported an error: {}); fail closed",
                "x".repeat(120 - "Engine reported an error: ".len())
            )
        );
    }
}
