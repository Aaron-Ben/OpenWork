import { invoke } from '@tauri-apps/api/core'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'

import type {
  RuntimeLoadedSession,
  RuntimeModelInput,
  RuntimeSessionInput,
  RuntimeSessionRecord,
  RuntimeSessionSnapshot,
  RuntimeSessionUpdateEnvelope,
  RuntimeTraceSpan,
  RuntimeTraceSummary,
  RuntimeTurnAccepted,
} from '../type/runtime'

export const runtimeApi = {
  upsertModel: (input: RuntimeModelInput): Promise<void> =>
    invoke('runtime_model_upsert', { input }),
  listSessions: (): Promise<RuntimeSessionRecord[]> => invoke('runtime_session_list'),
  createSession: (input: RuntimeSessionInput): Promise<RuntimeSessionRecord> =>
    invoke('runtime_session_create', { input }),
  loadSession: (sessionId: string): Promise<RuntimeLoadedSession> =>
    invoke('runtime_session_load', { sessionId }),
  renameSession: (sessionId: string, title: string): Promise<RuntimeSessionRecord> =>
    invoke('runtime_session_rename', { sessionId, title }),
  deleteSession: (sessionId: string): Promise<void> =>
    invoke('runtime_session_delete', { sessionId }),
  startTurn: (sessionId: string, clientRequestId: string, text: string): Promise<RuntimeTurnAccepted> =>
    invoke('runtime_turn_start', { sessionId, clientRequestId, text }),
  cancelTurn: (sessionId: string, turnId: string): Promise<boolean> =>
    invoke('runtime_turn_cancel', { sessionId, turnId }),
  resolvePermission: (
    sessionId: string,
    turnId: string,
    toolCallId: string,
    allow: boolean,
  ): Promise<void> =>
    invoke('runtime_permission_resolve', { sessionId, turnId, toolCallId, allow }),
  snapshot: (sessionId: string): Promise<RuntimeSessionSnapshot> =>
    invoke('runtime_session_snapshot', { sessionId }),
  replayUpdates: (sessionId: string, afterSequence: number): Promise<RuntimeSessionUpdateEnvelope[]> =>
    invoke('runtime_update_replay', { sessionId, afterSequence }),
  listTraces: (sessionId?: string, limit = 100): Promise<RuntimeTraceSummary[]> =>
    invoke('runtime_trace_list', { sessionId, limit }),
  getTrace: (turnId: string): Promise<RuntimeTraceSpan[]> =>
    invoke('runtime_trace_get', { turnId }),
  listenToUpdates: (
    handler: (payload: RuntimeSessionUpdateEnvelope) => void,
  ): Promise<UnlistenFn> =>
    listen<RuntimeSessionUpdateEnvelope>('session-update', (event) => handler(event.payload)),
}
