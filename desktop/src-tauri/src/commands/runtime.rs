use std::sync::Arc;

use openwork_core::{
    session::TurnId, ClientRequestId, ContextWindowInspection, ConversationCompaction,
    ConversationProjectionRecord, ConversationProjectionSelector, ConversationTranscriptPage,
    ConversationTranscriptQuery, LoadedSession, OpenWorkCore as OpenWorkCoreService,
    PermissionDecision, PermissionMode, ReapplyFileChangesResult, SessionId, SessionInput,
    SessionRecord, SessionSnapshot, SessionUpdateEnvelope, ToolCallId, TracePayloadSlot,
    TraceSpanPayloadRecord, TraceSpanRecord, TraceTurnSummary, TurnAccepted, TurnTrace,
    UndoFileChangesResult,
};

type OpenWorkCore = Arc<OpenWorkCoreService>;

use crate::CommandError;

#[tauri::command]
pub async fn runtime_session_list(
    core: tauri::State<'_, OpenWorkCore>,
) -> Result<Vec<SessionRecord>, CommandError> {
    core.list_sessions().await.map_err(CommandError::from)
}

#[tauri::command]
pub async fn runtime_sub_agent_list(
    core: tauri::State<'_, OpenWorkCore>,
    parent_session_id: String,
) -> Result<Vec<SessionRecord>, CommandError> {
    core.list_sub_agent_sessions(&SessionId::new(parent_session_id))
        .await
        .map_err(CommandError::from)
}

#[tauri::command]
pub async fn runtime_session_create(
    core: tauri::State<'_, OpenWorkCore>,
    input: SessionInput,
) -> Result<SessionRecord, CommandError> {
    core.create_session(&input)
        .await
        .map_err(CommandError::from)
}

#[tauri::command]
pub async fn runtime_session_load(
    core: tauri::State<'_, OpenWorkCore>,
    session_id: String,
) -> Result<LoadedSession, CommandError> {
    core.load_session(&SessionId::new(session_id))
        .await
        .map_err(CommandError::from)
}

#[tauri::command]
pub async fn runtime_context_window_inspect(
    core: tauri::State<'_, OpenWorkCore>,
    session_id: String,
) -> Result<ContextWindowInspection, CommandError> {
    core.inspect_context_window(&SessionId::new(session_id))
        .await
        .map_err(CommandError::from)
}

#[tauri::command]
pub async fn runtime_session_compact(
    core: tauri::State<'_, OpenWorkCore>,
    session_id: String,
) -> Result<ConversationCompaction, CommandError> {
    core.compact_conversation(&SessionId::new(session_id))
        .await
        .map_err(CommandError::from)
}

#[tauri::command]
pub async fn runtime_session_rewind(
    core: tauri::State<'_, OpenWorkCore>,
    session_id: String,
    compaction_id: String,
) -> Result<ConversationCompaction, CommandError> {
    core.rewind_conversation(&SessionId::new(session_id), &compaction_id)
        .await
        .map_err(CommandError::from)
}

#[tauri::command]
pub async fn runtime_compaction_list(
    core: tauri::State<'_, OpenWorkCore>,
    session_id: String,
) -> Result<Vec<ConversationCompaction>, CommandError> {
    core.list_conversation_compactions(&SessionId::new(session_id))
        .await
        .map_err(CommandError::from)
}

#[tauri::command]
pub async fn runtime_conversation_replay(
    core: tauri::State<'_, OpenWorkCore>,
    session_id: String,
    selector: ConversationProjectionSelector,
) -> Result<ConversationProjectionRecord, CommandError> {
    core.replay_conversation(&SessionId::new(session_id), selector)
        .await
        .map_err(CommandError::from)
}

#[tauri::command]
pub async fn runtime_compaction_transcript_read(
    core: tauri::State<'_, OpenWorkCore>,
    session_id: String,
    query: ConversationTranscriptQuery,
) -> Result<ConversationTranscriptPage, CommandError> {
    core.read_compaction_transcript(&SessionId::new(session_id), query)
        .await
        .map_err(CommandError::from)
}

#[tauri::command]
pub async fn runtime_session_rename(
    core: tauri::State<'_, OpenWorkCore>,
    session_id: String,
    title: String,
) -> Result<SessionRecord, CommandError> {
    core.rename_session(&SessionId::new(session_id), &title)
        .await
        .map_err(CommandError::from)
}

#[tauri::command]
pub async fn runtime_session_delete(
    core: tauri::State<'_, OpenWorkCore>,
    session_id: String,
) -> Result<(), CommandError> {
    core.delete_session(&SessionId::new(session_id))
        .await
        .map_err(CommandError::from)
}

#[tauri::command]
pub async fn runtime_turn_start(
    core: tauri::State<'_, OpenWorkCore>,
    session_id: String,
    client_request_id: String,
    input: Vec<openwork_core::UserInput>,
    context_window_tokens: Option<u64>,
) -> Result<TurnAccepted, CommandError> {
    let session_id = SessionId::new(session_id);
    let client_request_id = ClientRequestId::new(client_request_id);
    core.start_turn(&session_id, client_request_id, input, context_window_tokens)
        .await
        .map_err(CommandError::from)
}

#[tauri::command]
pub async fn runtime_turn_cancel(
    core: tauri::State<'_, OpenWorkCore>,
    session_id: String,
    turn_id: String,
) -> Result<bool, CommandError> {
    core.cancel_turn(&SessionId::new(session_id), TurnId::new(turn_id))
        .await
        .map_err(CommandError::from)
}

#[tauri::command]
pub async fn runtime_file_changes_undo(
    core: tauri::State<'_, OpenWorkCore>,
    session_id: String,
    change_ids: Vec<String>,
) -> Result<UndoFileChangesResult, CommandError> {
    core.undo_file_changes(&SessionId::new(session_id), change_ids)
        .await
        .map_err(CommandError::from)
}

#[tauri::command]
pub async fn runtime_file_changes_reapply(
    core: tauri::State<'_, OpenWorkCore>,
    session_id: String,
    change_ids: Vec<String>,
) -> Result<ReapplyFileChangesResult, CommandError> {
    core.reapply_file_changes(&SessionId::new(session_id), change_ids)
        .await
        .map_err(CommandError::from)
}

#[tauri::command]
pub async fn runtime_permission_resolve(
    core: tauri::State<'_, OpenWorkCore>,
    session_id: String,
    turn_id: String,
    tool_call_id: String,
    decision: PermissionDecision,
) -> Result<(), CommandError> {
    core.resolve_permission(
        &SessionId::new(session_id),
        TurnId::new(turn_id),
        ToolCallId::new(tool_call_id),
        decision,
    )
    .await
    .map_err(CommandError::from)
}

#[tauri::command]
pub async fn runtime_permission_mode_set(
    core: tauri::State<'_, OpenWorkCore>,
    session_id: String,
    mode: PermissionMode,
) -> Result<PermissionMode, CommandError> {
    core.set_permission_mode(&SessionId::new(session_id), mode)
        .await
        .map_err(CommandError::from)
}

#[tauri::command]
pub async fn runtime_session_snapshot(
    core: tauri::State<'_, OpenWorkCore>,
    session_id: String,
) -> Result<SessionSnapshot, CommandError> {
    core.get_session_snapshot(&SessionId::new(session_id))
        .await
        .map_err(CommandError::from)
}

#[tauri::command]
pub async fn runtime_update_replay(
    core: tauri::State<'_, OpenWorkCore>,
    session_id: String,
    after_sequence: u64,
) -> Result<Vec<SessionUpdateEnvelope>, CommandError> {
    core.replay_updates(&SessionId::new(session_id), after_sequence)
        .await
        .map_err(CommandError::from)
}

#[tauri::command]
pub async fn runtime_trace_list(
    core: tauri::State<'_, OpenWorkCore>,
    session_id: Option<String>,
    limit: i64,
) -> Result<Vec<TraceTurnSummary>, CommandError> {
    let session_id = session_id.map(SessionId::new);
    core.list_traces(session_id.as_ref(), limit)
        .await
        .map_err(CommandError::from)
}

#[tauri::command]
pub async fn runtime_trace_get(
    core: tauri::State<'_, OpenWorkCore>,
    turn_id: String,
) -> Result<TurnTrace, CommandError> {
    core.get_trace(&TurnId::new(turn_id))
        .await
        .map_err(CommandError::from)
}

#[tauri::command]
pub async fn runtime_trace_get_by_id(
    core: tauri::State<'_, OpenWorkCore>,
    trace_id: String,
) -> Result<TurnTrace, CommandError> {
    core.get_trace_by_id(&trace_id)
        .await
        .map_err(CommandError::from)
}

#[tauri::command]
pub async fn runtime_trace_payload_get(
    core: tauri::State<'_, OpenWorkCore>,
    span_id: String,
    slot: TracePayloadSlot,
) -> Result<Option<TraceSpanPayloadRecord>, CommandError> {
    core.get_span_payload(&span_id, slot)
        .await
        .map_err(CommandError::from)
}

/// Compaction Spans for one Session, newest first. Manual compactions have no
/// Turn and never appear in `runtime_trace_get`.
#[tauri::command]
pub async fn runtime_trace_compactions(
    core: tauri::State<'_, OpenWorkCore>,
    session_id: String,
    limit: i64,
) -> Result<Vec<TraceSpanRecord>, CommandError> {
    core.list_compaction_spans(&SessionId::new(session_id), limit)
        .await
        .map_err(CommandError::from)
}
