use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use async_trait::async_trait;
use openwork_protocol::capability::{
    ActionInvokeError, ActionInvoker, ActionRequest, CapabilityResolveError,
    CapabilityResolverPort, CapabilityRiskHint, CapabilitySpec, ExecutionPort, Observation,
    ObservationErrorCode, ObservationStatus,
};
use serde_json::json;

use openwork_execution::ExecutionService;

struct FakeResolver;

#[async_trait]
impl CapabilityResolverPort for FakeResolver {
    async fn list(&self) -> Result<Vec<CapabilitySpec>, CapabilityResolveError> {
        Ok(vec![self.spec()])
    }

    async fn resolve(&self, name: &str) -> Result<Option<CapabilitySpec>, CapabilityResolveError> {
        Ok((name == "write").then(|| self.spec()))
    }
}

impl FakeResolver {
    fn spec(&self) -> CapabilitySpec {
        CapabilitySpec {
            name: "write".to_string(),
            description: "Write a file".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "path": {"type": "string"},
                    "content": {"type": "string"}
                },
                "required": ["path", "content"]
            }),
            risk_hint: CapabilityRiskHint::WorkspaceMutation,
        }
    }
}

struct CountingInvoker {
    calls: Arc<AtomicUsize>,
}

#[async_trait]
impl ActionInvoker for CountingInvoker {
    async fn invoke(&self, _request: ActionRequest) -> Result<Observation, ActionInvokeError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(Observation::succeeded("written"))
    }
}

fn service(calls: Arc<AtomicUsize>) -> ExecutionService {
    ExecutionService::new(Arc::new(FakeResolver), Arc::new(CountingInvoker { calls }))
}

#[tokio::test]
async fn invalid_arguments_are_rejected_before_invocation() {
    let calls = Arc::new(AtomicUsize::new(0));
    let observation = service(Arc::clone(&calls))
        .execute(ActionRequest::new("write", json!({"path": "a.txt"})))
        .await;

    assert_eq!(observation.status, ObservationStatus::Failed);
    assert_eq!(
        observation.error.expect("error details").code,
        ObservationErrorCode::InvalidArguments
    );
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn unknown_capability_is_normalized_without_invocation() {
    let calls = Arc::new(AtomicUsize::new(0));
    let observation = service(Arc::clone(&calls))
        .execute(ActionRequest::new("missing", json!({})))
        .await;

    assert_eq!(observation.status, ObservationStatus::Failed);
    assert_eq!(
        observation.error.expect("error details").code,
        ObservationErrorCode::CapabilityNotFound
    );
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn valid_request_invokes_handler_once() {
    let calls = Arc::new(AtomicUsize::new(0));
    let observation = service(Arc::clone(&calls))
        .execute(ActionRequest::new(
            "write",
            json!({"path": "a.txt", "content": "hello"}),
        ))
        .await;

    assert_eq!(observation.status, ObservationStatus::Succeeded);
    assert_eq!(observation.text_content(), "written");
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}
