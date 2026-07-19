use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use futures_util::stream;
use openwork_agent::{AgentBuilder, AgentDefinition};
use openwork_chat_state::ChatStateHandle;
use openwork_core::session::{
    ClientRequestId, NoopTraceRecorder, PermissionDecision, ResolvedModel, SessionError,
    SessionHandle, SessionId, SessionRuntimeConfig, SessionStorage, SessionUpdate,
    SessionUpdateEnvelope, ToolCallId, ToolProgressUpdate, TurnId, TurnOutcome,
};
use openwork_models::model::{
    ContentBlock, FinishReason, Message, ModelCallOptions, ModelError, ModelEvent, ModelPort,
    ModelRequest, ModelResponse, ModelStream, Role, TokenUsage, ToolCallBlock, ToolCallState,
    ToolResultArtifact,
};
use openwork_tools::{
    PermissionMode, PermissionProfile, Tool, ToolCallContext, ToolExecutionError, ToolId,
    ToolInvocation, ToolProgress as RuntimeToolProgress, ToolRegistryBuilder, ToolResult, ToolRisk,
    ToolSessionContext,
};
use tokio::sync::broadcast;

#[derive(Default)]
struct ModelState {
    responses: Mutex<VecDeque<ModelResponse>>,
    requests: Mutex<Vec<ModelRequest>>,
}

struct FakeModel {
    state: Arc<ModelState>,
}

#[async_trait]
impl ModelPort for FakeModel {
    async fn invoke(
        &self,
        request: ModelRequest,
        _options: ModelCallOptions,
    ) -> Result<ModelStream, ModelError> {
        self.state.requests.lock().unwrap().push(request);
        let response = self
            .state
            .responses
            .lock()
            .unwrap()
            .pop_front()
            .ok_or_else(|| ModelError::protocol("fake model has no response"))?;
        let mut events = Vec::new();
        if !response.text.is_empty() {
            events.push(Ok(ModelEvent::TextDelta {
                index: 0,
                delta: response.text.clone(),
            }));
        }
        events.push(Ok(ModelEvent::ResponseCompleted {
            response: Box::new(response),
        }));
        Ok(Box::pin(stream::iter(events)))
    }
}

#[derive(Default)]
struct ToolState {
    invocations: Mutex<Vec<ToolInvocation>>,
    results: Mutex<VecDeque<ToolResult>>,
}

struct FakeTool {
    id: ToolId,
    risk: ToolRisk,
    state: Arc<ToolState>,
}

#[async_trait]
impl Tool for FakeTool {
    type Input = serde_json::Value;
    type Output = ToolResult;

    fn id(&self) -> ToolId {
        self.id.clone()
    }

    fn description(&self) -> &'static str {
        "Fake tool used by the session runtime integration tests."
    }

    fn risk(&self) -> ToolRisk {
        self.risk
    }

    async fn execute(
        &self,
        _session: &ToolSessionContext,
        call: ToolCallContext,
        input: serde_json::Value,
    ) -> Result<ToolResult, ToolExecutionError> {
        let wait_for_cancel = input["waitForCancel"] == true;
        let progress_message = input["emitProgress"].as_str().map(str::to_string);
        self.state
            .invocations
            .lock()
            .unwrap()
            .push(ToolInvocation::new(self.id.to_string(), input));
        if let Some(message) = progress_message {
            call.report_progress(RuntimeToolProgress::Message { message });
        }
        if wait_for_cancel {
            call.cancel.cancelled().await;
            return Ok(ToolResult::cancelled("fake tool cancelled"));
        }
        Ok(self
            .state
            .results
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or_else(|| ToolResult::succeeded("tool ok")))
    }
}

#[derive(Default)]
struct RecordingStorage {
    events: Mutex<Vec<String>>,
    fail_assistant: bool,
}

#[async_trait]
impl SessionStorage for RecordingStorage {
    async fn begin_turn(
        &self,
        _session_id: &SessionId,
        _turn_id: &TurnId,
        _client_request_id: &ClientRequestId,
        _model: &ResolvedModel,
        _user_message: &Message,
    ) -> Result<(), String> {
        self.events.lock().unwrap().push("begin_turn".to_string());
        Ok(())
    }

    async fn begin_model_call(
        &self,
        _turn_id: &TurnId,
        model_call_index: u32,
    ) -> Result<(), String> {
        self.events
            .lock()
            .unwrap()
            .push(format!("model_{model_call_index}"));
        Ok(())
    }

    async fn append_assistant_message(
        &self,
        _turn_id: &TurnId,
        _message: &Message,
        _usage: Option<TokenUsage>,
    ) -> Result<(), String> {
        self.events.lock().unwrap().push("assistant".to_string());
        if self.fail_assistant {
            Err("assistant write failed".to_string())
        } else {
            Ok(())
        }
    }

    async fn append_tool_result(
        &self,
        _turn_id: &TurnId,
        _message: &Message,
    ) -> Result<(), String> {
        self.events.lock().unwrap().push("tool_result".to_string());
        Ok(())
    }

    async fn finish_turn(&self, _turn_id: &TurnId, _outcome: &TurnOutcome) -> Result<(), String> {
        self.events.lock().unwrap().push("finish_turn".to_string());
        Ok(())
    }
}

struct RuntimeFixture {
    handle: SessionHandle,
    updates: broadcast::Receiver<SessionUpdateEnvelope>,
    global_updates: broadcast::Receiver<SessionUpdateEnvelope>,
    model: Arc<ModelState>,
    tools: Arc<ToolState>,
    storage: Arc<RecordingStorage>,
    chat: ChatStateHandle,
}

fn runtime(
    responses: Vec<ModelResponse>,
    tool_results: Vec<ToolResult>,
    permission_mode: PermissionMode,
    fail_assistant: bool,
) -> RuntimeFixture {
    let mut definition = AgentDefinition::default();
    definition.policy.permission_mode = permission_mode;
    let agent = AgentBuilder::new(definition).build().expect("agent");
    let chat = ChatStateHandle::spawn(Vec::new()).expect("chat");
    let model = Arc::new(ModelState {
        responses: Mutex::new(responses.into()),
        requests: Mutex::new(Vec::new()),
    });
    let tools = Arc::new(ToolState {
        invocations: Mutex::new(Vec::new()),
        results: Mutex::new(tool_results.into()),
    });
    let storage = Arc::new(RecordingStorage {
        events: Mutex::new(Vec::new()),
        fail_assistant,
    });
    let registry = AgentDefinition::default().tool_names.into_iter().fold(
        ToolRegistryBuilder::new(),
        |registry, name| {
            let risk = if name == "bash" {
                ToolRisk::ProcessExecution
            } else if matches!(name.as_str(), "write" | "edit") {
                ToolRisk::WorkspaceMutation
            } else {
                ToolRisk::ReadOnly
            };
            registry.register(FakeTool {
                id: ToolId::new(name),
                risk,
                state: Arc::clone(&tools),
            })
        },
    );
    let toolset = registry
        .finalize(
            agent.toolset_config(),
            ToolSessionContext::local(
                std::env::temp_dir(),
                PermissionProfile::danger_full_access(),
            ),
        )
        .expect("toolset");
    let (global_update_tx, global_updates) = broadcast::channel(512);
    let handle = SessionHandle::spawn_with_global_updates(
        SessionRuntimeConfig {
            session_id: SessionId::new("session-test"),
            resolved_model: ResolvedModel::new(None::<String>, "test", "test-model"),
            agent,
            chat: chat.clone(),
            model: Arc::new(FakeModel {
                state: Arc::clone(&model),
            }),
            tools: Arc::new(toolset),
            storage: storage.clone(),
            trace: Arc::new(NoopTraceRecorder),
        },
        global_update_tx,
    );
    let updates = handle.subscribe_updates();
    RuntimeFixture {
        handle,
        updates,
        global_updates,
        model,
        tools,
        storage,
        chat,
    }
}

#[tokio::test]
async fn session_actor_forwards_updates_to_the_core_global_bus() {
    let mut fixture = runtime(
        vec![response("done", Vec::new())],
        Vec::new(),
        PermissionMode::NeverAsk,
        false,
    );

    start(&fixture).await;
    let outcome = wait_for_terminal(&mut fixture.global_updates).await;

    assert!(matches!(outcome, TurnOutcome::Completed { .. }));
}

fn response(text: &str, tool_calls: Vec<ToolCallBlock>) -> ModelResponse {
    ModelResponse {
        response_id: Some("response".to_string()),
        provider_request_id: Some("request".to_string()),
        model: Some("test-model".to_string()),
        text: text.to_string(),
        reasoning_text: None,
        tool_calls,
        provider_opaque_blocks: Vec::new(),
        finish_reason: if text.is_empty() {
            FinishReason::ToolUse
        } else {
            FinishReason::Stop
        },
        raw_finish_reason: None,
        usage: None,
    }
}

fn tool_call(id: &str, name: &str, input: &str) -> ToolCallBlock {
    ToolCallBlock {
        id: id.to_string(),
        name: name.to_string(),
        input: input.to_string(),
        state: ToolCallState::Submitted,
    }
}

async fn start(fixture: &RuntimeFixture) -> TurnId {
    fixture
        .handle
        .start_turn(
            ClientRequestId::new("client-request"),
            vec![ContentBlock::text("do the task")],
        )
        .await
        .expect("turn accepted")
        .turn_id
}

async fn wait_for_terminal(
    updates: &mut broadcast::Receiver<SessionUpdateEnvelope>,
) -> TurnOutcome {
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        loop {
            let event = updates.recv().await.expect("session update");
            if let SessionUpdate::TurnFinished { outcome } = event.update {
                return outcome;
            }
        }
    })
    .await
    .expect("turn timed out")
}

async fn wait_for_permission(
    updates: &mut broadcast::Receiver<SessionUpdateEnvelope>,
) -> (TurnId, ToolCallId) {
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        loop {
            let event = updates.recv().await.expect("session update");
            if let SessionUpdate::PermissionRequested { request } = event.update {
                return (request.turn_id, request.tool_call_id);
            }
        }
    })
    .await
    .expect("permission timed out")
}

async fn wait_for_tool_start(updates: &mut broadcast::Receiver<SessionUpdateEnvelope>) {
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        loop {
            let event = updates.recv().await.expect("session update");
            if matches!(event.update, SessionUpdate::ToolCallStarted { .. }) {
                return;
            }
        }
    })
    .await
    .expect("tool start timed out");
}

#[tokio::test]
async fn no_tool_turn_completes_after_one_model_call() {
    let mut fixture = runtime(
        vec![response("done", Vec::new())],
        Vec::new(),
        PermissionMode::NeverAsk,
        false,
    );
    start(&fixture).await;

    assert_eq!(
        wait_for_terminal(&mut fixture.updates).await,
        TurnOutcome::Completed {
            final_text: "done".to_string()
        }
    );
    assert_eq!(fixture.model.requests.lock().unwrap().len(), 1);
    assert!(fixture.tools.invocations.lock().unwrap().is_empty());
    assert_eq!(
        *fixture.storage.events.lock().unwrap(),
        ["begin_turn", "model_1", "assistant", "finish_turn"]
    );
}

#[tokio::test]
async fn tool_result_is_in_the_next_model_request() {
    let mut fixture = runtime(
        vec![
            response(
                "",
                vec![tool_call("call-1", "read", r#"{"path":"README.md"}"#)],
            ),
            response("final", Vec::new()),
        ],
        vec![ToolResult::succeeded("file contents")],
        PermissionMode::NeverAsk,
        false,
    );
    start(&fixture).await;
    let outcome = wait_for_terminal(&mut fixture.updates).await;

    assert!(matches!(outcome, TurnOutcome::Completed { .. }));
    assert_eq!(fixture.tools.invocations.lock().unwrap().len(), 1);
    let requests = fixture.model.requests.lock().unwrap();
    assert_eq!(requests.len(), 2);
    assert!(
        requests[1]
            .messages
            .iter()
            .any(|message| message.role == Role::Tool)
    );
    assert_eq!(
        *fixture.storage.events.lock().unwrap(),
        [
            "begin_turn",
            "model_1",
            "assistant",
            "tool_result",
            "model_2",
            "assistant",
            "finish_turn"
        ]
    );
}

#[tokio::test]
async fn tool_result_artifacts_are_persisted_in_messages_and_forwarded_live() {
    let artifact = ToolResultArtifact {
        kind: "file_change".to_string(),
        payload: serde_json::json!({"changeId": "change-1", "path": "README.md"}),
    };
    let mut result = ToolResult::succeeded("edited README.md");
    result.artifacts.push(artifact.clone());
    let mut fixture = runtime(
        vec![
            response(
                "",
                vec![tool_call(
                    "call-artifact",
                    "write",
                    r#"{"path":"README.md","content":"new"}"#,
                )],
            ),
            response("final", Vec::new()),
        ],
        vec![result],
        PermissionMode::NeverAsk,
        false,
    );
    start(&fixture).await;

    let live_artifacts = tokio::time::timeout(std::time::Duration::from_secs(2), async {
        loop {
            let event = fixture.updates.recv().await.expect("session update");
            if let SessionUpdate::ToolCallFinished { artifacts, .. } = event.update {
                break artifacts;
            }
        }
    })
    .await
    .expect("artifact update timed out");
    assert_eq!(live_artifacts.as_slice(), std::slice::from_ref(&artifact));

    let _ = wait_for_terminal(&mut fixture.updates).await;
    let requests = fixture.model.requests.lock().unwrap();
    let persisted_artifacts = requests[1]
        .messages
        .iter()
        .flat_map(|message| &message.content)
        .find_map(|block| match block {
            ContentBlock::ToolResult(result) => Some(result.artifacts.as_slice()),
            _ => None,
        })
        .expect("tool result in next request");
    assert_eq!(persisted_artifacts, [artifact]);
}

#[tokio::test]
async fn tool_progress_is_forwarded_before_the_terminal_tool_update() {
    let mut fixture = runtime(
        vec![
            response(
                "",
                vec![tool_call(
                    "call-progress",
                    "read",
                    r#"{"emitProgress":"scanning"}"#,
                )],
            ),
            response("final", Vec::new()),
        ],
        Vec::new(),
        PermissionMode::NeverAsk,
        false,
    );
    start(&fixture).await;

    let (progress_sequence, finished_sequence) =
        tokio::time::timeout(std::time::Duration::from_secs(2), async {
            let mut progress_sequence = None;
            loop {
                let event = fixture.updates.recv().await.expect("session update");
                match event.update {
                    SessionUpdate::ToolCallProgress {
                        progress: ToolProgressUpdate::Message { message },
                        ..
                    } => {
                        assert_eq!(message, "scanning");
                        progress_sequence = Some(event.sequence);
                    }
                    SessionUpdate::ToolCallFinished { .. } => {
                        break (progress_sequence.expect("progress update"), event.sequence);
                    }
                    _ => {}
                }
            }
        })
        .await
        .expect("tool updates timed out");

    assert!(progress_sequence < finished_sequence);
}

#[tokio::test]
async fn cancelling_a_turn_cancels_the_active_tool_call() {
    let mut fixture = runtime(
        vec![response(
            "",
            vec![tool_call("call-1", "read", r#"{"waitForCancel":true}"#)],
        )],
        Vec::new(),
        PermissionMode::NeverAsk,
        false,
    );
    let turn_id = start(&fixture).await;
    wait_for_tool_start(&mut fixture.updates).await;
    assert!(
        fixture
            .handle
            .cancel_turn(turn_id)
            .await
            .expect("cancel turn")
    );

    assert!(matches!(
        wait_for_terminal(&mut fixture.updates).await,
        TurnOutcome::Cancelled
    ));
}

#[tokio::test]
async fn unknown_tool_becomes_a_result_and_the_model_continues() {
    let mut fixture = runtime(
        vec![
            response("", vec![tool_call("call-1", "missing", "{}")]),
            response("recovered", Vec::new()),
        ],
        Vec::new(),
        PermissionMode::NeverAsk,
        false,
    );
    start(&fixture).await;

    assert!(matches!(
        wait_for_terminal(&mut fixture.updates).await,
        TurnOutcome::Completed { .. }
    ));
    assert!(fixture.tools.invocations.lock().unwrap().is_empty());
    assert_eq!(fixture.model.requests.lock().unwrap().len(), 2);
}

#[tokio::test]
async fn permission_allow_executes_the_tool_and_finishes() {
    let mut fixture = runtime(
        vec![
            response(
                "",
                vec![tool_call("call-1", "read", r#"{"path":"README.md"}"#)],
            ),
            response("done", Vec::new()),
        ],
        Vec::new(),
        PermissionMode::Ask,
        false,
    );
    start(&fixture).await;
    let (turn_id, tool_call_id) = wait_for_permission(&mut fixture.updates).await;
    fixture
        .handle
        .resolve_permission(turn_id, tool_call_id, PermissionDecision::Allow)
        .await
        .expect("permission");

    assert!(matches!(
        wait_for_terminal(&mut fixture.updates).await,
        TurnOutcome::Completed { .. }
    ));
    assert_eq!(fixture.tools.invocations.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn permission_deny_writes_a_tool_result_without_execution() {
    let mut fixture = runtime(
        vec![response(
            "",
            vec![tool_call("call-1", "bash", r#"{"command":"pwd"}"#)],
        )],
        Vec::new(),
        PermissionMode::Ask,
        false,
    );
    start(&fixture).await;
    let (turn_id, tool_call_id) = wait_for_permission(&mut fixture.updates).await;
    fixture
        .handle
        .resolve_permission(turn_id, tool_call_id, PermissionDecision::Deny)
        .await
        .expect("permission");

    assert!(matches!(
        wait_for_terminal(&mut fixture.updates).await,
        TurnOutcome::Failed { code, .. } if code == "permission_denied"
    ));
    assert!(fixture.tools.invocations.lock().unwrap().is_empty());
    let snapshot = fixture.chat.snapshot().await.expect("chat snapshot");
    assert!(
        snapshot
            .messages
            .iter()
            .any(|message| message.role == Role::Tool)
    );
}

#[tokio::test]
async fn assistant_persistence_failure_prevents_tool_execution() {
    let mut fixture = runtime(
        vec![response(
            "",
            vec![tool_call("call-1", "read", r#"{"path":"README.md"}"#)],
        )],
        Vec::new(),
        PermissionMode::NeverAsk,
        true,
    );
    start(&fixture).await;

    assert!(matches!(
        wait_for_terminal(&mut fixture.updates).await,
        TurnOutcome::Failed { code, .. } if code == "persistence_error"
    ));
    assert!(fixture.tools.invocations.lock().unwrap().is_empty());
}

#[tokio::test]
async fn tool_failure_is_returned_to_the_model_instead_of_stopping_the_loop() {
    let mut fixture = runtime(
        vec![
            response(
                "",
                vec![tool_call("call-1", "read", r#"{"path":"missing.txt"}"#)],
            ),
            response("used another approach", Vec::new()),
        ],
        vec![ToolResult::failed(
            openwork_tools::ToolErrorCode::ExecutionFailed,
            "not found",
            false,
        )],
        PermissionMode::NeverAsk,
        false,
    );
    start(&fixture).await;

    assert!(matches!(
        wait_for_terminal(&mut fixture.updates).await,
        TurnOutcome::Completed { .. }
    ));
    assert_eq!(fixture.model.requests.lock().unwrap().len(), 2);
    assert_eq!(fixture.tools.invocations.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn multiple_tool_results_keep_provider_order_in_the_next_request() {
    let mut fixture = runtime(
        vec![
            response(
                "",
                vec![
                    tool_call("call-1", "read", r#"{"path":"a"}"#),
                    tool_call("call-2", "read", r#"{"path":"b"}"#),
                ],
            ),
            response("done", Vec::new()),
        ],
        vec![ToolResult::succeeded("a"), ToolResult::succeeded("b")],
        PermissionMode::NeverAsk,
        false,
    );
    start(&fixture).await;
    wait_for_terminal(&mut fixture.updates).await;

    let requests = fixture.model.requests.lock().unwrap();
    let tool_ids: Vec<_> = requests[1]
        .messages
        .iter()
        .filter(|message| message.role == Role::Tool)
        .flat_map(|message| &message.content)
        .filter_map(|block| match block {
            ContentBlock::ToolResult(result) => Some(result.id.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(tool_ids, ["call-1", "call-2"]);
}

#[tokio::test]
async fn duplicate_client_request_is_idempotent_and_a_different_turn_is_busy() {
    let mut fixture = runtime(
        vec![response(
            "",
            vec![tool_call("call-1", "read", r#"{"path":"a"}"#)],
        )],
        Vec::new(),
        PermissionMode::Ask,
        false,
    );
    let first_turn = start(&fixture).await;
    let (turn_id, tool_call_id) = wait_for_permission(&mut fixture.updates).await;

    let duplicate = fixture
        .handle
        .start_turn(
            ClientRequestId::new("client-request"),
            vec![ContentBlock::text("same retry")],
        )
        .await
        .expect("idempotent retry");
    assert_eq!(duplicate.turn_id, first_turn);

    let busy = fixture
        .handle
        .start_turn(
            ClientRequestId::new("another-request"),
            vec![ContentBlock::text("overlap")],
        )
        .await;
    assert!(matches!(busy, Err(SessionError::Busy(id)) if id == first_turn));

    fixture
        .handle
        .resolve_permission(turn_id, tool_call_id, PermissionDecision::Deny)
        .await
        .expect("cleanup permission");
    wait_for_terminal(&mut fixture.updates).await;
}
