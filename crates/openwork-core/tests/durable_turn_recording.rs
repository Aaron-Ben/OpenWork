use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};

use async_trait::async_trait;
use futures_util::stream;
use openwork_core::{Agent, AgentConfig, AgentError, AgentPorts, turn_command_channel};
use openwork_protocol::{
    approval::{ApprovalPolicy, ApprovalRequested, ApprovalResolution, ExecutionPolicyDecision},
    capability::{
        ActionRequest, CapabilityResolveError, CapabilityResolverPort, CapabilitySpec,
        ExecutionPort, Observation,
    },
    domain::{ApprovalId, StepId, ToolRunId, TurnId},
    model::{
        ContentBlock, FinishReason, Message, ModelCallOptions, ModelError, ModelEvent, ModelPort,
        ModelRequest, ModelResponse, ModelStream, Role, ToolCallBlock, ToolCallState,
    },
    turn::{TurnRecordError, TurnRecordedEvent, TurnRecorderPort},
};
use serde_json::json;
use tokio_util::sync::CancellationToken;

struct OneToolProvider;

#[async_trait]
impl ModelPort for OneToolProvider {
    async fn invoke(
        &self,
        _request: ModelRequest,
        _options: ModelCallOptions,
    ) -> Result<ModelStream, ModelError> {
        let response = ModelResponse {
            response_id: None,
            provider_request_id: None,
            model: Some("fake".to_string()),
            text: String::new(),
            reasoning_text: None,
            tool_calls: vec![ToolCallBlock {
                id: "provider-call-1".to_string(),
                name: "bash".to_string(),
                input: json!({"command": "touch must-not-run"}).to_string(),
                state: ToolCallState::Submitted,
            }],
            provider_opaque_blocks: Vec::new(),
            finish_reason: FinishReason::ToolUse,
            raw_finish_reason: Some("tool_use".to_string()),
            usage: None,
        };
        Ok(Box::pin(stream::iter(vec![Ok(
            ModelEvent::ResponseCompleted {
                response: Box::new(response),
            },
        )])))
    }
}

struct TextProvider;

#[async_trait]
impl ModelPort for TextProvider {
    async fn invoke(
        &self,
        _request: ModelRequest,
        _options: ModelCallOptions,
    ) -> Result<ModelStream, ModelError> {
        let response = ModelResponse {
            response_id: None,
            provider_request_id: None,
            model: Some("fake".to_string()),
            text: "done".to_string(),
            reasoning_text: None,
            tool_calls: Vec::new(),
            provider_opaque_blocks: Vec::new(),
            finish_reason: FinishReason::Stop,
            raw_finish_reason: Some("stop".to_string()),
            usage: None,
        };
        Ok(Box::pin(stream::iter(vec![Ok(
            ModelEvent::ResponseCompleted {
                response: Box::new(response),
            },
        )])))
    }
}

struct FakeCapabilities;

#[async_trait]
impl CapabilityResolverPort for FakeCapabilities {
    async fn list(&self) -> Result<Vec<CapabilitySpec>, CapabilityResolveError> {
        Ok(Vec::new())
    }

    async fn resolve(&self, _name: &str) -> Result<Option<CapabilitySpec>, CapabilityResolveError> {
        Ok(None)
    }
}

struct CountingExecution {
    calls: Arc<AtomicUsize>,
}

#[async_trait]
impl ExecutionPort for CountingExecution {
    async fn authorize(
        &self,
        _request: &ActionRequest,
        _policy: ApprovalPolicy,
    ) -> ExecutionPolicyDecision {
        ExecutionPolicyDecision::Allow
    }

    async fn execute(&self, _request: ActionRequest) -> Observation {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Observation::succeeded("executed")
    }
}

struct FailingRecorder {
    seen: Arc<Mutex<Vec<&'static str>>>,
}

struct CollectingRecorder {
    seen: Arc<Mutex<Vec<&'static str>>>,
}

#[async_trait]
impl TurnRecorderPort for CollectingRecorder {
    async fn append(&self, events: Vec<TurnRecordedEvent>) -> Result<(), TurnRecordError> {
        self.seen
            .lock()
            .expect("recorder mutex")
            .extend(events.iter().map(TurnRecordedEvent::event_type));
        Ok(())
    }
}

#[async_trait]
impl TurnRecorderPort for FailingRecorder {
    async fn append(&self, events: Vec<TurnRecordedEvent>) -> Result<(), TurnRecordError> {
        let mut seen = self.seen.lock().expect("recorder mutex");
        for event in events {
            let event_type = event.event_type();
            seen.push(event_type);
            if event_type == "tool_run_started" {
                return Err(TurnRecordError::Unavailable {
                    message: "database offline".to_string(),
                });
            }
        }
        Ok(())
    }
}

#[tokio::test]
async fn a_tool_is_not_invoked_when_its_started_fact_cannot_be_recorded() {
    let turn_id = TurnId::new("turn-1");
    let (_handle, inbox) = turn_command_channel(turn_id.clone());
    let calls = Arc::new(AtomicUsize::new(0));
    let seen = Arc::new(Mutex::new(Vec::new()));
    let recorder: Arc<dyn TurnRecorderPort> = Arc::new(FailingRecorder {
        seen: Arc::clone(&seen),
    });
    let config = AgentConfig::new(
        Box::new(OneToolProvider),
        "fake",
        AgentPorts::new(
            Arc::new(FakeCapabilities),
            Arc::new(CountingExecution {
                calls: Arc::clone(&calls),
            }),
            recorder,
        ),
        turn_id,
        inbox,
        CancellationToken::new(),
    );
    let mut agent = Agent::new(config);

    let result = agent
        .run(vec![Message::text(Role::User, "run it")], |_| {})
        .await;

    assert!(matches!(result, Err(AgentError::Record(_))));
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert_eq!(
        *seen.lock().expect("recorder mutex"),
        vec![
            "step_started",
            "assistant_message_recorded",
            "tool_run_requested",
            "tool_run_started"
        ]
    );
}

#[tokio::test]
async fn a_durably_pending_approval_can_resume_the_tool_and_next_model_step() {
    let turn_id = TurnId::new("turn-1");
    let (_handle, inbox) = turn_command_channel(turn_id.clone());
    let calls = Arc::new(AtomicUsize::new(0));
    let seen = Arc::new(Mutex::new(Vec::new()));
    let recorder: Arc<dyn TurnRecorderPort> = Arc::new(CollectingRecorder {
        seen: Arc::clone(&seen),
    });
    let config = AgentConfig::new(
        Box::new(TextProvider),
        "fake",
        AgentPorts::new(
            Arc::new(FakeCapabilities),
            Arc::new(CountingExecution {
                calls: Arc::clone(&calls),
            }),
            recorder,
        ),
        turn_id.clone(),
        inbox,
        CancellationToken::new(),
    );
    let mut agent = Agent::new(config);
    let step_id = StepId::new("step-1");
    let tool_run_id = ToolRunId::new("tool-run-1");
    let tool_call = ToolCallBlock {
        id: "provider-call-1".to_string(),
        name: "bash".to_string(),
        input: json!({"command": "cargo test"}).to_string(),
        state: ToolCallState::Submitted,
    };
    let history = vec![
        Message::text(Role::User, "run tests"),
        Message {
            role: Role::Assistant,
            content: vec![ContentBlock::ToolCall(tool_call)],
        },
    ];
    let recovery = openwork_core::ApprovalRecovery {
        request: ApprovalRequested {
            approval_id: ApprovalId::new("approval-1"),
            turn_id,
            step_id,
            tool_run_id,
            tool_name: "bash".to_string(),
            input: json!({"command": "cargo test"}),
            reason: "process execution requires approval".to_string(),
        },
        provider_tool_call_id: "provider-call-1".to_string(),
        step_index: 1,
    };

    let result = agent
        .resume_after_approval(history, recovery, ApprovalResolution::Allow, |_| {})
        .await
        .expect("recovered turn finishes");

    assert_eq!(result.text, "done");
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        *seen.lock().expect("recorder mutex"),
        vec![
            "approval_resolved",
            "tool_run_started",
            "tool_run_completed",
            "tool_message_recorded",
            "step_completed",
            "step_started",
            "assistant_message_recorded",
            "step_completed",
        ]
    );
}
