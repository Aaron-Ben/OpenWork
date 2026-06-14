use anvil_core::ai::{GenerateRequest, GenerateStreamEvent, Message, Role};
use anvil_providers::{
    build_provider, test_provider, ProviderConfig, ProviderIndex, ProviderInput, ProviderPreset,
    ProviderStore, TestResult, BUILTIN_PRESETS,
};
use serde::{Deserialize, Serialize};
use tauri::{Emitter, Manager};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ChatGenerateRequest {
    provider_id: String,
    model: String,
    messages: Vec<ChatInputMessage>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ChatGenerateStreamRequest {
    request_id: String,
    provider_id: String,
    model: String,
    messages: Vec<ChatInputMessage>,
}

#[derive(Debug, Deserialize)]
struct ChatInputMessage {
    role: ChatRole,
    content: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
enum ChatRole {
    User,
    Assistant,
    System,
}

impl From<ChatRole> for Role {
    fn from(value: ChatRole) -> Self {
        match value {
            ChatRole::User => Role::User,
            ChatRole::Assistant => Role::Assistant,
            ChatRole::System => Role::System,
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ChatGenerateResponse {
    text: String,
    reasoning_text: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ChatStreamEventPayload {
    request_id: String,
    event: &'static str,
    delta: Option<String>,
    message: Option<String>,
}

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

#[tauri::command]
async fn chat_generate(
    store: tauri::State<'_, ProviderStore>,
    request: ChatGenerateRequest,
) -> Result<ChatGenerateResponse, String> {
    let config = store
        .get(&request.provider_id)
        .ok_or_else(|| format!("provider not found: {}", request.provider_id))?;

    let provider = build_provider(&config);
    let messages = request
        .messages
        .into_iter()
        .map(|message| Message::text(message.role.into(), message.content))
        .collect();

    let generate_request = GenerateRequest {
        model: request.model,
        messages,
        temperature: None,
        max_tokens: None,
        stream: false,
        thinking: None,
    };

    let response = provider
        .generate(generate_request)
        .await
        .map_err(|error| error.to_string())?;

    Ok(ChatGenerateResponse {
        text: response.text,
        reasoning_text: response.reasoning_text,
    })
}

#[tauri::command]
async fn chat_generate_stream(
    app: tauri::AppHandle,
    store: tauri::State<'_, ProviderStore>,
    request: ChatGenerateStreamRequest,
) -> Result<ChatGenerateResponse, String> {
    let request_id = request.request_id;
    let config = store
        .get(&request.provider_id)
        .ok_or_else(|| format!("provider not found: {}", request.provider_id))?;

    let provider = build_provider(&config);
    let generate_request = GenerateRequest {
        model: request.model,
        messages: request
            .messages
            .into_iter()
            .map(|message| Message::text(message.role.into(), message.content))
            .collect(),
        temperature: None,
        max_tokens: None,
        stream: true,
        thinking: None,
    };

    let event_app = app.clone();
    let event_request_id = request_id.clone();
    let response = provider
        .stream_generate(
            generate_request,
            Box::new(move |event| {
                let (event_name, delta) = match event {
                    GenerateStreamEvent::TextDelta { delta } => ("text_delta", delta),
                    GenerateStreamEvent::ReasoningDelta { delta } => ("reasoning_delta", delta),
                };
                let _ = event_app.emit(
                    "chat-stream-event",
                    ChatStreamEventPayload {
                        request_id: event_request_id.clone(),
                        event: event_name,
                        delta: Some(delta),
                        message: None,
                    },
                );
            }),
        )
        .await;

    match response {
        Ok(response) => {
            let _ = app.emit(
                "chat-stream-event",
                ChatStreamEventPayload {
                    request_id: request_id.clone(),
                    event: "done",
                    delta: None,
                    message: None,
                },
            );
            Ok(ChatGenerateResponse {
                text: response.text,
                reasoning_text: response.reasoning_text,
            })
        }
        Err(error) => {
            let message = error.to_string();
            let _ = app.emit(
                "chat-stream-event",
                ChatStreamEventPayload {
                    request_id,
                    event: "error",
                    delta: None,
                    message: Some(message.clone()),
                },
            );
            Err(message)
        }
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let app_data_dir = app.path().app_data_dir()?;
            let store = ProviderStore::open(app_data_dir.join("providers.json"));
            app.manage(store);
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
            chat_generate,
            chat_generate_stream,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chat_role_maps_to_core_role() {
        assert_eq!(Role::from(ChatRole::User), Role::User);
        assert_eq!(Role::from(ChatRole::Assistant), Role::Assistant);
        assert_eq!(Role::from(ChatRole::System), Role::System);
    }

    #[test]
    fn builtin_presets_exclude_local_models() {
        let ids: Vec<&str> = BUILTIN_PRESETS.iter().map(|preset| preset.id).collect();
        assert!(!ids.contains(&"ollama"));
        assert!(!ids.contains(&"lmstudio"));
        assert!(!ids.contains(&"official"));
    }
}
