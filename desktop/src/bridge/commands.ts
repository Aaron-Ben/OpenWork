import { invoke } from '@tauri-apps/api/core'

import type {
  RuntimeConversationCompaction,
  RuntimeConversationProjection,
  RuntimeConversationProjectionSelector,
  RuntimeConversationTranscriptPage,
  RuntimeConversationTranscriptQuery,
  RuntimeContextWindowInspection,
  RuntimeLoadedSession,
  RuntimePermissionMode,
  RuntimePermissionDecision,
  RuntimeReapplyFileChangesResult,
  RuntimeSessionInput,
  RuntimeSessionRecord,
  RuntimeSessionSnapshot,
  RuntimeSessionUpdateEnvelope,
  RuntimeSubAgentSessionRecord,
  RuntimeSkillDetail,
  RuntimeSkillDiscovery,
  RuntimeUserInput,
  RuntimeTracePayloadSlot,
  RuntimeTraceSpan,
  RuntimeTraceSpanPayload,
  RuntimeTraceSummary,
  RuntimeTurnTrace,
  RuntimeTurnAccepted,
  RuntimeUndoFileChangesResult,
} from './compat'

export const coreCommands = {
  listSkills: (): Promise<RuntimeSkillDiscovery> => invoke('list_skills'),
  setSkillDisabled: (name: string, disabled: boolean): Promise<RuntimeSkillDiscovery> =>
    invoke('set_skill_disabled', { name, disabled }),
  readSkill: (path: string): Promise<RuntimeSkillDetail> => invoke('read_skill', { path }),
  listSessions: (): Promise<RuntimeSessionRecord[]> => invoke('runtime_session_list'),
  listSubAgents: (parentSessionId: string): Promise<RuntimeSubAgentSessionRecord[]> =>
    invoke('runtime_sub_agent_list', { parentSessionId }),
  createSession: (input: RuntimeSessionInput): Promise<RuntimeSessionRecord> =>
    invoke('runtime_session_create', { input }),
  loadSession: (sessionId: string): Promise<RuntimeLoadedSession> =>
    invoke('runtime_session_load', { sessionId }),
  inspectContextWindow: (sessionId: string): Promise<RuntimeContextWindowInspection> =>
    invoke('runtime_context_window_inspect', { sessionId }),
  compactConversation: (sessionId: string): Promise<RuntimeConversationCompaction> =>
    invoke('runtime_session_compact', { sessionId }),
  rewindConversation: (
    sessionId: string,
    compactionId: string,
  ): Promise<RuntimeConversationCompaction> =>
    invoke('runtime_session_rewind', { sessionId, compactionId }),
  listCompactions: (sessionId: string): Promise<RuntimeConversationCompaction[]> =>
    invoke('runtime_compaction_list', { sessionId }),
  replayConversation: (
    sessionId: string,
    selector: RuntimeConversationProjectionSelector,
  ): Promise<RuntimeConversationProjection> =>
    invoke('runtime_conversation_replay', { sessionId, selector }),
  readCompactionTranscript: (
    sessionId: string,
    query: RuntimeConversationTranscriptQuery = {},
  ): Promise<RuntimeConversationTranscriptPage> =>
    invoke('runtime_compaction_transcript_read', { sessionId, query }),
  renameSession: (sessionId: string, title: string): Promise<RuntimeSessionRecord> =>
    invoke('runtime_session_rename', { sessionId, title }),
  deleteSession: (sessionId: string): Promise<void> =>
    invoke('runtime_session_delete', { sessionId }),
  startTurn: (
    sessionId: string,
    clientRequestId: string,
    input: RuntimeUserInput[],
  ): Promise<RuntimeTurnAccepted> =>
    invoke('runtime_turn_start', { sessionId, clientRequestId, input }),
  cancelTurn: (sessionId: string, turnId: string): Promise<boolean> =>
    invoke('runtime_turn_cancel', { sessionId, turnId }),
  undoFileChanges: (
    sessionId: string,
    changeIds: string[],
  ): Promise<RuntimeUndoFileChangesResult> =>
    invoke('runtime_file_changes_undo', { sessionId, changeIds }),
  reapplyFileChanges: (
    sessionId: string,
    changeIds: string[],
  ): Promise<RuntimeReapplyFileChangesResult> =>
    invoke('runtime_file_changes_reapply', { sessionId, changeIds }),
  resolvePermission: (
    sessionId: string,
    turnId: string,
    toolCallId: string,
    decision: RuntimePermissionDecision,
  ): Promise<void> =>
    invoke('runtime_permission_resolve', { sessionId, turnId, toolCallId, decision }),
  setPermissionMode: (
    sessionId: string,
    mode: RuntimePermissionMode,
  ): Promise<RuntimePermissionMode> =>
    invoke('runtime_permission_mode_set', { sessionId, mode }),
  snapshot: (sessionId: string): Promise<RuntimeSessionSnapshot> =>
    invoke('runtime_session_snapshot', { sessionId }),
  replayUpdates: (
    sessionId: string,
    afterSequence: number,
  ): Promise<RuntimeSessionUpdateEnvelope[]> =>
    invoke('runtime_update_replay', { sessionId, afterSequence }),
  listTraces: (sessionId?: string, limit = 100): Promise<RuntimeTraceSummary[]> =>
    invoke('runtime_trace_list', { sessionId, limit }),
  getTrace: (turnId: string): Promise<RuntimeTurnTrace> =>
    invoke('runtime_trace_get', { turnId }),
  getTraceById: (traceId: string): Promise<RuntimeTurnTrace> =>
    invoke('runtime_trace_get_by_id', { traceId }),
  getSpanPayload: (
    spanId: string,
    slot: RuntimeTracePayloadSlot,
  ): Promise<RuntimeTraceSpanPayload | null> =>
    invoke('runtime_trace_payload_get', { spanId, slot }),
  listCompactionSpans: (sessionId: string, limit = 50): Promise<RuntimeTraceSpan[]> =>
    invoke('runtime_trace_compactions', { sessionId, limit }),
}
