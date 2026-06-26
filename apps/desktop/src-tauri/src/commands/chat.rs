use openwork_runtime::{
    ApprovalBridge, ApprovalDecision, ChatGenerateRequest, ChatGenerateResponse, ChatRuntime,
    RequestCancelRegistry,
};
use tauri::Emitter;

#[tauri::command]
pub async fn chat_generate_stream(
    app: tauri::AppHandle,
    runtime: tauri::State<'_, ChatRuntime>,
    approval_bridge: tauri::State<'_, ApprovalBridge>,
    cancel_registry: tauri::State<'_, RequestCancelRegistry>,
    request: ChatGenerateRequest,
) -> Result<ChatGenerateResponse, String> {
    let request_id = request.request_id.clone();
    let cancel = cancel_registry.register(&request_id);
    let result = runtime
        .generate_stream(
            request,
            approval_bridge.inner().clone(),
            cancel,
            move |payload| {
                let _ = app.emit("chat-stream-event", payload);
            },
        )
        .await
        .map_err(|error| error.to_string());
    cancel_registry.remove(&request_id);
    result
}

#[tauri::command]
pub async fn resolve_approval(
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

#[tauri::command]
pub async fn chat_abort(
    cancel_registry: tauri::State<'_, RequestCancelRegistry>,
    approval_bridge: tauri::State<'_, ApprovalBridge>,
    request_id: String,
) -> Result<bool, String> {
    approval_bridge.cancel_all().await;
    Ok(cancel_registry.cancel(&request_id))
}
