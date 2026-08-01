import { create } from 'zustand'

import type {
  RuntimePermissionMode,
  RuntimeSessionSnapshot,
  RuntimeSessionUpdateEnvelope,
} from '../../bridge/compat'
import {
  createSessionRuntimeView,
  reduceSessionUpdate,
  runtimeViewFromSnapshot,
  type SessionRuntimeView,
} from './runtimeReducer'

export interface RuntimeApplyResult {
  duplicate: boolean
  sequenceGap: boolean
  terminal: boolean
  draftCleared: boolean
}

interface RuntimeStoreState {
  bySession: Record<string, SessionRuntimeView>
  beginTurn: (sessionId: string, clientRequestId: string, text: string) => boolean
  acceptTurn: (sessionId: string, clientRequestId: string, turnId: string) => void
  failTurnStart: (sessionId: string, clientRequestId: string, message: string) => void
  apply: (envelope: RuntimeSessionUpdateEnvelope) => RuntimeApplyResult
  replaceSnapshot: (snapshot: RuntimeSessionSnapshot) => void
  markResyncing: (sessionId: string) => void
  markSyncFailed: (sessionId: string, message: string) => void
  setError: (sessionId: string, message: string | null) => void
  setPermissionMode: (sessionId: string, mode: RuntimePermissionMode) => void
  reconcileCanonical: (sessionId: string) => void
  clearSession: (sessionId: string) => void
}

function viewFor(state: RuntimeStoreState, sessionId: string): SessionRuntimeView {
  return state.bySession[sessionId] ?? createSessionRuntimeView()
}

export const useRuntimeStore = create<RuntimeStoreState>((set, get) => ({
  bySession: {},

  beginTurn: (sessionId, clientRequestId, text) => {
    const current = viewFor(get(), sessionId)
    if (current.phase !== 'idle' || !text.trim()) return false
    set((state) => ({
      bySession: {
        ...state.bySession,
        [sessionId]: {
          ...current,
          clientRequestId,
          turnId: null,
          phase: 'starting',
          pendingUserMessage: {
            id: `optimistic-${clientRequestId}`,
            clientRequestId,
            turnId: null,
            text: text.trim(),
            state: 'pending',
            error: null,
          },
          assistantDraft: null,
          toolCalls: {},
          orderedToolCallIds: [],
          pendingPermission: null,
          terminal: null,
          error: null,
        },
      },
    }))
    return true
  },

  acceptTurn: (sessionId, clientRequestId, turnId) => {
    set((state) => {
      const current = viewFor(state, sessionId)
      if (current.clientRequestId !== clientRequestId) return state
      return {
        bySession: {
          ...state.bySession,
          [sessionId]: {
            ...current,
            turnId,
            pendingUserMessage: current.pendingUserMessage
              ? { ...current.pendingUserMessage, turnId }
              : null,
          },
        },
      }
    })
  },

  failTurnStart: (sessionId, clientRequestId, message) => {
    set((state) => {
      const current = viewFor(state, sessionId)
      if (current.clientRequestId !== clientRequestId) return state
      return {
        bySession: {
          ...state.bySession,
          [sessionId]: {
            ...current,
            phase: 'idle',
            error: message,
            pendingUserMessage: current.pendingUserMessage
              ? { ...current.pendingUserMessage, state: 'failed', error: message }
              : null,
          },
        },
      }
    })
  },

  apply: (envelope) => {
    const previous = viewFor(get(), envelope.sessionId)
    const duplicate = envelope.sequence <= previous.lastSequence
    const sequenceGap = envelope.sequence > previous.lastSequence + 1
    const next = reduceSessionUpdate(previous, envelope)
    if (next !== previous) {
      set((state) => ({
        bySession: { ...state.bySession, [envelope.sessionId]: next },
      }))
    }
    return {
      duplicate,
      sequenceGap,
      terminal: envelope.update.type === 'turn_finished',
      draftCleared: envelope.update.type === 'draft_cleared',
    }
  },

  replaceSnapshot: (snapshot) => {
    set((state) => ({
      bySession: {
        ...state.bySession,
        [snapshot.sessionId]: runtimeViewFromSnapshot(snapshot),
      },
    }))
  },

  markResyncing: (sessionId) => {
    set((state) => ({
      bySession: {
        ...state.bySession,
        [sessionId]: { ...viewFor(state, sessionId), syncState: 'resyncing' },
      },
    }))
  },

  markSyncFailed: (sessionId, message) => {
    set((state) => ({
      bySession: {
        ...state.bySession,
        [sessionId]: {
          ...viewFor(state, sessionId),
          syncState: 'stale',
          error: message,
        },
      },
    }))
  },

  setError: (sessionId, message) => {
    set((state) => ({
      bySession: {
        ...state.bySession,
        [sessionId]: { ...viewFor(state, sessionId), error: message },
      },
    }))
  },

  setPermissionMode: (sessionId, mode) => {
    set((state) => ({
      bySession: {
        ...state.bySession,
        [sessionId]: { ...viewFor(state, sessionId), permissionMode: mode },
      },
    }))
  },

  reconcileCanonical: (sessionId) => {
    set((state) => {
      const current = viewFor(state, sessionId)
      const terminalReconciled = current.phase === 'idle' && current.terminal !== null
      return {
        bySession: {
          ...state.bySession,
          [sessionId]: {
            ...current,
            pendingUserMessage: null,
            assistantDraft: null,
            ...(terminalReconciled
              ? {
                  turnId: null,
                  clientRequestId: null,
                  toolCalls: {},
                  orderedToolCallIds: [],
                  pendingPermission: null,
                  terminal: null,
                  error: null,
                }
              : {}),
          },
        },
      }
    })
  },

  clearSession: (sessionId) => {
    set((state) => {
      const bySession = { ...state.bySession }
      delete bySession[sessionId]
      return { bySession }
    })
  },
}))

export const EMPTY_RUNTIME_VIEW = createSessionRuntimeView()
