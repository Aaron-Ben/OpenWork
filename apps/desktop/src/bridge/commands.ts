import { invoke } from '@tauri-apps/api/core'

import type {
  RuntimeContextWindowInspection,
  RuntimeLoadedSession,
  RuntimeReapplyFileChangesResult,
  RuntimeSessionInput,
  RuntimeSessionRecord,
  RuntimeSessionSnapshot,
  RuntimeSessionUpdateEnvelope,
  RuntimeTraceSummary,
  RuntimeTurnTrace,
  RuntimeTurnAccepted,
  RuntimeUndoFileChangesResult,
} from './compat'

export const coreCommands = {
  listSessions: (): Promise<RuntimeSessionRecord[]> => invoke('runtime_session_list'),
  createSession: (input: RuntimeSessionInput): Promise<RuntimeSessionRecord> =>
    invoke('runtime_session_create', { input }),
  loadSession: (sessionId: string): Promise<RuntimeLoadedSession> =>
    invoke('runtime_session_load', { sessionId }),
  inspectContextWindow: (sessionId: string): Promise<RuntimeContextWindowInspection> =>
    invoke('runtime_context_window_inspect', { sessionId }),
  renameSession: (sessionId: string, title: string): Promise<RuntimeSessionRecord> =>
    invoke('runtime_session_rename', { sessionId, title }),
  deleteSession: (sessionId: string): Promise<void> =>
    invoke('runtime_session_delete', { sessionId }),
  startTurn: (
    sessionId: string,
    clientRequestId: string,
    text: string,
  ): Promise<RuntimeTurnAccepted> =>
    invoke('runtime_turn_start', { sessionId, clientRequestId, text }),
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
    allow: boolean,
  ): Promise<void> =>
    invoke('runtime_permission_resolve', { sessionId, turnId, toolCallId, allow }),
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
}
