use std::path::PathBuf;

use anvil_core::ai::{ContentBlock, Message, Role};
use anvil_providers::{
    build_provider, test_provider, ProviderConfig, ProviderIndex, ProviderInput, ProviderPreset,
    ProviderStore, TestResult, BUILTIN_PRESETS,
};
use anvil_runtime::{Agent, AgentConfig, AgentEvent, ApprovalBridge, ApprovalDecision, ApprovalPolicy};
use anvil_session::{NewMessage, Session, SessionInput, SessionLoadResult, SessionStore, SessionSummary};
use serde::{Deserialize, Serialize};
use tauri::{Emitter, Manager};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ChatGenerateStreamRequest {
    request_id: String,
    session_id: String,
    provider_id: String,
    model: String,
    user_text: String,
    approval_policy: Option<ApprovalPolicy>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ChatGenerateResponse {
    text: String,
    reasoning_text: Option<String>,
}

/// 前端 `chat-stream-event` 监听的单帧 payload。`session_id` 用于多会话隔离分派。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ChatStreamEventPayload {
    request_id: String,
    session_id: String,
    event: &'static str,
    delta: Option<String>,
    message: Option<String>,
    step: Option<usize>,
    tool_call_id: Option<String>,
    tool_name: Option<String>,
    partial_input: Option<String>,
    tool_output: Option<String>,
    is_error: Option<bool>,
    approval_id: Option<String>,
    input: Option<serde_json::Value>,
}

impl ChatStreamEventPayload {
    fn simple(request_id: &str, session_id: &str, event: &'static str) -> Self {
        Self {
            request_id: request_id.to_string(),
            session_id: session_id.to_string(),
            event,
            delta: None,
            message: None,
            step: None,
            tool_call_id: None,
            tool_name: None,
            partial_input: None,
            tool_output: None,
            is_error: None,
            approval_id: None,
            input: None,
        }
    }
}

// ---------------------------------------------------------------------------
// Provider 命令(不变)
// ---------------------------------------------------------------------------

#[tauri::command]
fn provider_list(store: tauri::State<'_, ProviderStore>) -> ProviderIndex {
    store.index()
}

#[tauri::command]
fn provider_presets() -> Vec<ProviderPreset> {
    BUILTIN_PRESETS.to_vec()
}

#[tauri::command]
fn provider_create(
    store: tauri::State<'_, ProviderStore>,
    input: ProviderInput,
) -> Result<ProviderConfig, String> {
    store.add(input).map_err(|error| error.to_string())
}

#[tauri::command]
fn provider_update(
    store: tauri::State<'_, ProviderStore>,
    id: String,
    input: ProviderInput,
) -> Result<ProviderConfig, String> {
    store.update(&id, input).map_err(|error| error.to_string())
}

#[tauri::command]
fn provider_delete(store: tauri::State<'_, ProviderStore>, id: String) -> Result<(), String> {
    store.delete(&id).map_err(|error| error.to_string())
}

#[tauri::command]
fn provider_activate(store: tauri::State<'_, ProviderStore>, id: String) -> Result<(), String> {
    store.activate(&id).map_err(|error| error.to_string())
}

#[tauri::command]
async fn provider_test(config: ProviderConfig, model: String) -> TestResult {
    test_provider(&config, &model).await
}

// ---------------------------------------------------------------------------
// Session 命令
// ---------------------------------------------------------------------------

#[tauri::command]
fn session_list(store: tauri::State<'_, SessionStore>) -> Result<Vec<SessionSummary>, String> {
    store.list_sessions().map_err(|error| error.to_string())
}

#[tauri::command]
fn session_create(
    store: tauri::State<'_, SessionStore>,
    input: SessionInput,
) -> Result<Session, String> {
    store.create_session(input).map_err(|error| error.to_string())
}

#[tauri::command]
fn session_load(
    store: tauri::State<'_, SessionStore>,
    id: String,
) -> Result<SessionLoadResult, String> {
    let session = store
        .load_session(&id)
        .map_err(|error| error.to_string())?
        .ok_or_else(|| format!("session not found: {id}"))?;
    let messages = store.load_messages(&id).map_err(|error| error.to_string())?;
    Ok(SessionLoadResult { session, messages })
}

#[tauri::command]
fn session_delete(store: tauri::State<'_, SessionStore>, id: String) -> Result<(), String> {
    store.delete_session(&id).map_err(|error| error.to_string())
}

#[tauri::command]
fn session_rename(
    store: tauri::State<'_, SessionStore>,
    id: String,
    title: String,
) -> Result<Session, String> {
    store
        .rename_session(&id, &title)
        .map_err(|error| error.to_string())
}

// ---------------------------------------------------------------------------
// 聊天(agent loop)—— session 驱动,历史从持久化层加载,run 成功后落库新增消息
// ---------------------------------------------------------------------------

#[tauri::command]
async fn chat_generate_stream(
    app: tauri::AppHandle,
    provider_store: tauri::State<'_, ProviderStore>,
    session_store: tauri::State<'_, SessionStore>,
    approval_bridge: tauri::State<'_, ApprovalBridge>,
    request: ChatGenerateStreamRequest,
) -> Result<ChatGenerateResponse, String> {
    let request_id = request.request_id.clone();
    let session_id = request.session_id.clone();

    let config = provider_store
        .get(&request.provider_id)
        .ok_or_else(|| format!("provider not found: {}", request.provider_id))?;
    let provider = build_provider(&config);

    // 从持久化层加载历史(role + parts 直接映射为 agent 的 Message)。
    let stored = session_store
        .load_messages(&session_id)
        .map_err(|error| error.to_string())?;
    let history_len = stored.len();
    let mut history: Vec<Message> = stored
        .into_iter()
        .map(|message| Message {
            role: message.role,
            content: message.parts,
        })
        .collect();
    // 追加本轮用户输入。
    history.push(Message::text(Role::User, &request.user_text));

    let working_dir = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let mut agent_config = AgentConfig::new(provider, request.model, working_dir);
    agent_config.approval_bridge = approval_bridge.inner().clone();
    if let Some(approval_policy) = request.approval_policy {
        agent_config.approval_policy = approval_policy;
    }
    let agent = Agent::new(agent_config);

    let event_app = app.clone();
    let event_request_id = request_id.clone();
    let event_session_id = session_id.clone();
    let result = agent
        .run(history, move |event| {
            let _ = event_app.emit(
                "chat-stream-event",
                map_agent_event(&event_request_id, &event_session_id, &event),
            );
        })
        .await;

    match result {
        Ok(run_result) => {
            // 持久化本轮新增消息(跳过加载来的历史,保留 user + assistant + tool)。
            let new_messages: Vec<NewMessage> = run_result
                .messages
                .into_iter()
                .skip(history_len)
                .map(|message| NewMessage {
                    role: message.role,
                    parts: message.content,
                })
                .collect();
            session_store
                .append_messages(&session_id, new_messages)
                .map_err(|error| error.to_string())?;

            let _ = app.emit(
                "chat-stream-event",
                ChatStreamEventPayload::simple(&request_id, &session_id, "done"),
            );
            Ok(ChatGenerateResponse {
                text: run_result.text,
                reasoning_text: None,
            })
        }
        Err(error) => {
            let message = error.to_string();
            let _ = app.emit(
                "chat-stream-event",
                ChatStreamEventPayload {
                    message: Some(message.clone()),
                    ..ChatStreamEventPayload::simple(&request_id, &session_id, "error")
                },
            );
            Err(message)
        }
    }
}

/// 前端审批弹窗回传决定:用 `approval_id` 匹配 agent loop 正在 await 的 pending 请求。
#[tauri::command]
async fn resolve_approval(
    approval_bridge: tauri::State<'_, ApprovalBridge>,
    approval_id: String,
    allow: bool,
) -> Result<(), String> {
    let decision = if allow {
        ApprovalDecision::Allow
    } else {
        ApprovalDecision::Deny("denied by user".to_string())
    };
    approval_bridge.resolve(&approval_id, decision).await
}

/// 把 agent loop 的事件映射成前端可消费的流式 payload。
fn map_agent_event(request_id: &str, session_id: &str, event: &AgentEvent) -> ChatStreamEventPayload {
    let mut payload = ChatStreamEventPayload::simple(request_id, session_id, "");
    match event {
        AgentEvent::Step(n) => {
            payload.event = "step";
            payload.step = Some(*n);
        }
        AgentEvent::TextDelta(delta) => {
            payload.event = "text_delta";
            payload.delta = Some(delta.clone());
        }
        AgentEvent::ReasoningDelta(delta) => {
            payload.event = "reasoning_delta";
            payload.delta = Some(delta.clone());
        }
        AgentEvent::ToolCallStart { id, name } => {
            payload.event = "tool_call_start";
            payload.tool_call_id = Some(id.clone());
            payload.tool_name = Some(name.clone());
        }
        AgentEvent::ToolCallDelta { id, partial_input } => {
            payload.event = "tool_call_delta";
            payload.tool_call_id = Some(id.clone());
            payload.partial_input = Some(partial_input.clone());
        }
        AgentEvent::ToolCallEnd { id } => {
            payload.event = "tool_call_end";
            payload.tool_call_id = Some(id.clone());
        }
        AgentEvent::ToolResult {
            id,
            name,
            output,
            is_error,
        } => {
            payload.event = "tool_result";
            payload.tool_call_id = Some(id.clone());
            payload.tool_name = Some(name.clone());
            payload.tool_output = Some(extract_text(output));
            payload.is_error = Some(*is_error);
        }
        AgentEvent::ApprovalRequest { id, name, input } => {
            payload.event = "approval_request";
            payload.approval_id = Some(id.clone());
            payload.tool_name = Some(name.clone());
            payload.input = Some(input.clone());
        }
        AgentEvent::Finished(text) => {
            payload.event = "finished";
            payload.delta = Some(text.clone());
        }
    }
    payload
}

fn extract_text(blocks: &[ContentBlock]) -> String {
    blocks
        .iter()
        .filter_map(|block| match block {
            ContentBlock::Text(text) => Some(text.text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let app_data_dir = app.path().app_data_dir()?;
            let provider_store = ProviderStore::open(app_data_dir.join("providers.json"));
            app.manage(provider_store);
            let session_store = SessionStore::open(app_data_dir.join("anvil.db"))?;
            app.manage(session_store);
            app.manage(ApprovalBridge::new());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            provider_list,
            provider_presets,
            provider_create,
            provider_update,
            provider_delete,
            provider_activate,
            provider_test,
            session_list,
            session_create,
            session_load,
            session_delete,
            session_rename,
            chat_generate_stream,
            resolve_approval,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(test)]
mod tests {
    use anvil_providers::BUILTIN_PRESETS;

    #[test]
    fn builtin_presets_exclude_local_models() {
        let ids: Vec<&str> = BUILTIN_PRESETS.iter().map(|preset| preset.id).collect();
        assert!(!ids.contains(&"ollama"));
        assert!(!ids.contains(&"lmstudio"));
        assert!(!ids.contains(&"official"));
    }
}
