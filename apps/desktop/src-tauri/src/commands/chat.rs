use openwork_app::{
    ApprovalResolution, ChatGenerateRequest, ChatGenerateResponse, ChatRuntime,
    RequestCancelRegistry, ResolveApproval,
};
use openwork_protocol::domain::{ApprovalId, TurnId};
use tauri::Emitter;

#[tauri::command]
pub async fn chat_generate_stream(
    app: tauri::AppHandle,
    runtime: tauri::State<'_, ChatRuntime>,
    cancel_registry: tauri::State<'_, RequestCancelRegistry>,
    request: ChatGenerateRequest,
) -> Result<ChatGenerateResponse, String> {
    let request_id = request.request_id.clone();
    let cancel = cancel_registry.register(&request_id);
    let result = runtime
        .generate_stream(request, cancel, move |payload| {
            let _ = app.emit("chat-stream-event", payload);
        })
        .await
        .map_err(|error| error.to_string());
    cancel_registry.remove(&request_id);
    result
}

#[tauri::command]
pub async fn resolve_approval(
    runtime: tauri::State<'_, ChatRuntime>,
    turn_id: String,
    approval_id: String,
    allow: bool,
) -> Result<(), String> {
    let resolution = if allow {
        ApprovalResolution::Allow
    } else {
        ApprovalResolution::Deny {
            reason: "denied by user".to_string(),
        }
    };
    runtime
        .resolve_approval(ResolveApproval {
            turn_id: TurnId::new(turn_id),
            approval_id: ApprovalId::new(approval_id),
            resolution,
        })
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn chat_abort(
    cancel_registry: tauri::State<'_, RequestCancelRegistry>,
    request_id: String,
) -> Result<bool, String> {
    Ok(cancel_registry.cancel(&request_id))
}
