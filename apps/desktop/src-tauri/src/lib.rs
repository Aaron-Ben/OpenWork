use std::collections::HashMap;
use std::collections::HashSet;
use std::fs;
use std::io;
use std::path::PathBuf;
use std::process::Command;
use std::sync::Mutex;

use anvil_core::ai::{ContentBlock, Message, Role};
use anvil_providers::{
    build_provider, test_provider, ProviderConfig, ProviderIndex, ProviderInput, ProviderPreset,
    ProviderStore, TestResult, BUILTIN_PRESETS,
};
use anvil_runtime::{
    Agent, AgentConfig, AgentError, AgentEvent, ApprovalBridge, ApprovalDecision, ApprovalPolicy,
};
use anvil_session::{
    NewMessage, NewWorktreeSnapshot, NewWorktreeSnapshotFile, Session, SessionInput,
    SessionLoadResult, SessionStore, SessionSummary, WorktreeSnapshotSummary,
};
use serde::{Deserialize, Serialize};
use tauri::{Emitter, Manager};
use tokio_util::sync::CancellationToken;

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

#[derive(Debug, Clone)]
struct CapturedWorktree {
    working_dir: PathBuf,
    status: Vec<GitStatusEntry>,
    contents: HashMap<String, Option<Vec<u8>>>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct RevertSnapshotResponse {
    snapshot: WorktreeSnapshotSummary,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct WorktreeSnapshotDetail {
    snapshot: WorktreeSnapshotSummary,
    files: Vec<WorktreeSnapshotFileDetail>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct WorktreeSnapshotFileDetail {
    path: String,
    before_text: Option<String>,
    after_text: Option<String>,
    binary: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct GitStatusEntry {
    path: String,
    status: String,
}

/// 按 request_id 注册的取消令牌表,供 `chat_abort` 触发对应请求的取消。
#[derive(Default)]
struct RequestCancelRegistry {
    tokens: Mutex<HashMap<String, CancellationToken>>,
}

impl RequestCancelRegistry {
    /// 注册一个 request_id,返回其取消令牌(克隆给 agent)。
    fn register(&self, request_id: &str) -> CancellationToken {
        let token = CancellationToken::new();
        self.tokens
            .lock()
            .expect("cancel registry mutex poisoned")
            .insert(request_id.to_string(), token.clone());
        token
    }

    /// 触发并移除一个 request_id 的令牌。返回是否命中。
    fn cancel(&self, request_id: &str) -> bool {
        if let Some(token) = self
            .tokens
            .lock()
            .expect("cancel registry mutex poisoned")
            .remove(request_id)
        {
            token.cancel();
            true
        } else {
            false
        }
    }

    /// 请求结束时移除令牌(正常完成 / 出错都要清,防泄漏)。
    fn remove(&self, request_id: &str) {
        self.tokens
            .lock()
            .expect("cancel registry mutex poisoned")
            .remove(request_id);
    }
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
    store
        .create_session(input)
        .map_err(|error| error.to_string())
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
    let messages = store
        .load_messages(&id)
        .map_err(|error| error.to_string())?;
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

#[tauri::command]
fn session_worktree_snapshots(
    store: tauri::State<'_, SessionStore>,
    session_id: String,
) -> Result<Vec<WorktreeSnapshotSummary>, String> {
    store
        .list_worktree_snapshots(&session_id)
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn session_worktree_snapshot_detail(
    store: tauri::State<'_, SessionStore>,
    snapshot_id: String,
) -> Result<WorktreeSnapshotDetail, String> {
    let snapshot = store
        .load_worktree_snapshot(&snapshot_id)
        .map_err(|error| error.to_string())?
        .ok_or_else(|| format!("snapshot not found: {snapshot_id}"))?;
    let files = snapshot
        .files
        .into_iter()
        .map(|file| {
            let before_text = bytes_to_text(file.before_content.as_deref());
            let after_text = bytes_to_text(file.after_content.as_deref());
            WorktreeSnapshotFileDetail {
                path: file.path,
                binary: before_text.is_none() && after_text.is_none(),
                before_text,
                after_text,
            }
        })
        .collect();
    Ok(WorktreeSnapshotDetail {
        snapshot: snapshot.summary,
        files,
    })
}

#[tauri::command]
fn session_revert_worktree_snapshot(
    store: tauri::State<'_, SessionStore>,
    snapshot_id: String,
) -> Result<RevertSnapshotResponse, String> {
    let snapshot = store
        .load_worktree_snapshot(&snapshot_id)
        .map_err(|error| error.to_string())?
        .ok_or_else(|| format!("snapshot not found: {snapshot_id}"))?;
    if snapshot.summary.reverted_at.is_some() {
        return Err("snapshot already reverted".to_string());
    }

    let working_dir = PathBuf::from(&snapshot.summary.working_dir);
    for file in &snapshot.files {
        let path = safe_worktree_path(&working_dir, &file.path)?;
        let current = read_optional(&path).map_err(|error| error.to_string())?;
        if current != file.after_content {
            return Err(format!(
                "cannot revert because file changed after snapshot: {}",
                file.path
            ));
        }
    }

    for file in &snapshot.files {
        let path = safe_worktree_path(&working_dir, &file.path)?;
        match &file.before_content {
            Some(bytes) => {
                if let Some(parent) = path.parent() {
                    fs::create_dir_all(parent).map_err(|error| error.to_string())?;
                }
                fs::write(&path, bytes).map_err(|error| error.to_string())?;
            }
            None => {
                if path.exists() {
                    fs::remove_file(&path).map_err(|error| error.to_string())?;
                }
            }
        }
    }

    store
        .mark_worktree_snapshot_reverted(&snapshot_id)
        .map_err(|error| error.to_string())?;
    let updated = store
        .load_worktree_snapshot(&snapshot_id)
        .map_err(|error| error.to_string())?
        .ok_or_else(|| format!("snapshot not found after revert: {snapshot_id}"))?;
    Ok(RevertSnapshotResponse {
        snapshot: updated.summary,
    })
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
    cancel_registry: tauri::State<'_, RequestCancelRegistry>,
    request: ChatGenerateStreamRequest,
) -> Result<ChatGenerateResponse, String> {
    let request_id = request.request_id.clone();
    let session_id = request.session_id.clone();

    let config = provider_store
        .get(&request.provider_id)
        .ok_or_else(|| format!("provider not found: {}", request.provider_id))?;
    let provider = build_provider(&config);

    let session = session_store
        .load_session(&session_id)
        .map_err(|error| error.to_string())?
        .ok_or_else(|| format!("session not found: {session_id}"))?;

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

    let working_dir = session
        .working_dir
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
    let before_worktree = capture_worktree(&working_dir).ok();
    let mut agent_config = AgentConfig::new(provider, request.model, working_dir);
    agent_config.approval_bridge = approval_bridge.inner().clone();
    agent_config.cancel = cancel_registry.register(&request_id);
    if let Some(approval_policy) = request.approval_policy {
        agent_config.approval_policy = approval_policy;
    }
    let agent = Agent::new(agent_config);

    let event_app = app.clone();
    let event_request_id = request_id.clone();
    let event_session_id = session_id.clone();
    let event_session_store = session_store.inner().clone();
    let result = agent
        .run(history, move |event| {
            let payload = map_agent_event(&event_request_id, &event_session_id, &event);
            if let Ok(value) = serde_json::to_value(&payload) {
                let _ = event_session_store.append_llm_event(
                    &event_session_id,
                    &event_request_id,
                    payload.event,
                    value,
                );
            }
            let _ = event_app.emit("chat-stream-event", payload);
        })
        .await;

    match result {
        Ok(run_result) => {
            cancel_registry.remove(&request_id);
            persist_worktree_snapshot(
                &session_store,
                &session_id,
                &request_id,
                before_worktree.as_ref(),
            );
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
        Err(AgentError::Cancelled(msgs)) => {
            cancel_registry.remove(&request_id);
            persist_worktree_snapshot(
                &session_store,
                &session_id,
                &request_id,
                before_worktree.as_ref(),
            );
            // 持久化截止取消时已生成的消息(若有),然后发 cancelled 事件(非 error)。
            let new_messages: Vec<NewMessage> = msgs
                .into_iter()
                .skip(history_len)
                .map(|message| NewMessage {
                    role: message.role,
                    parts: message.content,
                })
                .collect();
            let _ = session_store.append_messages(&session_id, new_messages);
            let _ = app.emit(
                "chat-stream-event",
                ChatStreamEventPayload::simple(&request_id, &session_id, "cancelled"),
            );
            Ok(ChatGenerateResponse {
                text: String::new(),
                reasoning_text: None,
            })
        }
        Err(AgentError::DoomLoop(_name, msgs)) => {
            cancel_registry.remove(&request_id);
            persist_worktree_snapshot(
                &session_store,
                &session_id,
                &request_id,
                before_worktree.as_ref(),
            );
            // doom_loop 事件已由 run 内的 DoomLoopDetected emit;这里只持久化部分消息。
            let new_messages: Vec<NewMessage> = msgs
                .into_iter()
                .skip(history_len)
                .map(|message| NewMessage {
                    role: message.role,
                    parts: message.content,
                })
                .collect();
            let _ = session_store.append_messages(&session_id, new_messages);
            Ok(ChatGenerateResponse {
                text: String::new(),
                reasoning_text: None,
            })
        }
        Err(error) => {
            cancel_registry.remove(&request_id);
            persist_worktree_snapshot(
                &session_store,
                &session_id,
                &request_id,
                before_worktree.as_ref(),
            );
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

/// 取消正在进行的 chat 请求:触发对应 request_id 的 CancellationToken,
/// 并清理所有 pending 审批(避免 agent 卡在审批 await)。
#[tauri::command]
async fn chat_abort(
    cancel_registry: tauri::State<'_, RequestCancelRegistry>,
    approval_bridge: tauri::State<'_, ApprovalBridge>,
    request_id: String,
) -> Result<bool, String> {
    approval_bridge.cancel_all().await;
    Ok(cancel_registry.cancel(&request_id))
}

fn persist_worktree_snapshot(
    session_store: &SessionStore,
    session_id: &str,
    request_id: &str,
    before: Option<&CapturedWorktree>,
) {
    let Some(before) = before else {
        return;
    };
    let Ok(after) = capture_worktree(&before.working_dir) else {
        return;
    };
    let paths = snapshot_paths(before, &after);
    if paths.is_empty() {
        return;
    }

    let mut changed_files = Vec::new();
    let mut files = Vec::new();
    for path in paths {
        let before_content = before.contents.get(&path).cloned().unwrap_or_else(|| {
            read_git_head_file(&before.working_dir, &path)
                .ok()
                .flatten()
        });
        let after_content = after.contents.get(&path).cloned().unwrap_or_else(|| {
            read_optional(&before.working_dir.join(&path))
                .ok()
                .flatten()
        });
        if before_content == after_content {
            continue;
        }
        changed_files.push(path.clone());
        files.push(NewWorktreeSnapshotFile {
            path,
            before_content,
            after_content,
        });
    }
    if changed_files.is_empty() {
        return;
    }

    changed_files.sort();
    files.sort_by(|a, b| a.path.cmp(&b.path));
    let snapshot = NewWorktreeSnapshot {
        request_id: request_id.to_string(),
        working_dir: before.working_dir.to_string_lossy().to_string(),
        before_status: serde_json::json!(before.status),
        after_status: serde_json::json!(after.status),
        changed_files,
        files,
    };
    let _ = session_store.append_worktree_snapshot(session_id, snapshot);
}

fn capture_worktree(working_dir: &std::path::Path) -> io::Result<CapturedWorktree> {
    let root = git_root(working_dir)?;
    let status = git_status(&root)?;
    let mut contents = HashMap::new();
    for entry in &status {
        let path = safe_worktree_path(&root, &entry.path)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?;
        contents.insert(entry.path.clone(), read_optional(&path)?);
    }
    Ok(CapturedWorktree {
        working_dir: root,
        status,
        contents,
    })
}

fn snapshot_paths(before: &CapturedWorktree, after: &CapturedWorktree) -> Vec<String> {
    let mut paths = HashSet::new();
    for entry in before.status.iter().chain(after.status.iter()) {
        paths.insert(entry.path.clone());
    }
    let mut out: Vec<String> = paths.into_iter().collect();
    out.sort();
    out
}

fn git_root(working_dir: &std::path::Path) -> io::Result<PathBuf> {
    let output = Command::new("git")
        .args([
            "-C",
            &working_dir.to_string_lossy(),
            "rev-parse",
            "--show-toplevel",
        ])
        .output()?;
    if !output.status.success() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "working directory is not inside a git repository",
        ));
    }
    let text = String::from_utf8_lossy(&output.stdout);
    Ok(PathBuf::from(text.trim()))
}

fn git_status(root: &std::path::Path) -> io::Result<Vec<GitStatusEntry>> {
    let output = Command::new("git")
        .args([
            "-C",
            &root.to_string_lossy(),
            "status",
            "--porcelain=v1",
            "-z",
        ])
        .output()?;
    if !output.status.success() {
        return Err(io::Error::other("git status failed"));
    }
    let mut entries = Vec::new();
    let mut parts = output
        .stdout
        .split(|byte| *byte == 0)
        .filter(|part| !part.is_empty());
    while let Some(raw) = parts.next() {
        if raw.len() < 4 {
            continue;
        }
        let status = String::from_utf8_lossy(&raw[..2]).to_string();
        let path = String::from_utf8_lossy(&raw[3..]).to_string();
        if status.starts_with('R') || status.starts_with('C') {
            let _old_path = parts.next();
        }
        entries.push(GitStatusEntry { path, status });
    }
    Ok(entries)
}

fn read_git_head_file(root: &std::path::Path, path: &str) -> io::Result<Option<Vec<u8>>> {
    let output = Command::new("git")
        .args([
            "-C",
            &root.to_string_lossy(),
            "show",
            &format!("HEAD:{path}"),
        ])
        .output()?;
    if output.status.success() {
        Ok(Some(output.stdout))
    } else {
        Ok(None)
    }
}

fn read_optional(path: &std::path::Path) -> io::Result<Option<Vec<u8>>> {
    match fs::read(path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

fn safe_worktree_path(root: &std::path::Path, relative: &str) -> Result<PathBuf, String> {
    let relative_path = std::path::Path::new(relative);
    if relative_path.is_absolute()
        || relative_path
            .components()
            .any(|component| matches!(component, std::path::Component::ParentDir))
    {
        return Err(format!("unsafe snapshot path: {relative}"));
    }
    Ok(root.join(relative_path))
}

fn bytes_to_text(bytes: Option<&[u8]>) -> Option<String> {
    let bytes = bytes?;
    if bytes.contains(&0) {
        return None;
    }
    String::from_utf8(bytes.to_vec()).ok()
}

/// 把 agent loop 的事件映射成前端可消费的流式 payload。
fn map_agent_event(
    request_id: &str,
    session_id: &str,
    event: &AgentEvent,
) -> ChatStreamEventPayload {
    let mut payload = ChatStreamEventPayload::simple(request_id, session_id, "");
    match event {
        AgentEvent::Step(n) => {
            payload.event = "step";
            payload.step = Some(*n);
        }
        AgentEvent::LlmStepStart { index } => {
            payload.event = "llm_step_start";
            payload.step = Some(*index);
        }
        AgentEvent::LlmStepFinish { index, reason, .. } => {
            payload.event = "llm_step_finish";
            payload.step = Some(*index);
            payload.message = Some(reason.clone());
        }
        AgentEvent::LlmFinish { reason, .. } => {
            payload.event = "llm_finish";
            payload.message = Some(reason.clone());
        }
        AgentEvent::TextStart { id } => {
            payload.event = "text_start";
            payload.message = Some(id.clone());
        }
        AgentEvent::TextDelta(delta) => {
            payload.event = "text_delta";
            payload.delta = Some(delta.clone());
        }
        AgentEvent::TextEnd { id } => {
            payload.event = "text_end";
            payload.message = Some(id.clone());
        }
        AgentEvent::ReasoningStart { id } => {
            payload.event = "reasoning_start";
            payload.message = Some(id.clone());
        }
        AgentEvent::ReasoningDelta(delta) => {
            payload.event = "reasoning_delta";
            payload.delta = Some(delta.clone());
        }
        AgentEvent::ReasoningEnd { id } => {
            payload.event = "reasoning_end";
            payload.message = Some(id.clone());
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
        AgentEvent::DoomLoopDetected { repeated } => {
            payload.event = "doom_loop";
            payload.message = Some(repeated.clone());
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
            app.manage(RequestCancelRegistry::default());
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
            session_worktree_snapshots,
            session_worktree_snapshot_detail,
            session_revert_worktree_snapshot,
            chat_generate_stream,
            resolve_approval,
            chat_abort,
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
