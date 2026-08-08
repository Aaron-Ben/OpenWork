import { create } from 'zustand'

import type {
  RuntimePermissionMode,
  RuntimeSessionSnapshot,
  RuntimeSessionUpdateEnvelope,
} from '@/bridge/compat'
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
  subAgentParentBySession: Record<string, string>
  beginTurn: (sessionId: string, clientRequestId: string, text: string) => boolean
  acceptTurn: (sessionId: string, clientRequestId: string, turnId: string) => void
  failTurnStart: (sessionId: string, clientRequestId: string, message: string) => void
  apply: (envelope: RuntimeSessionUpdateEnvelope) => RuntimeApplyResult
  applyBatch: (envelopes: RuntimeSessionUpdateEnvelope[]) => RuntimeApplyResult
  replaceSnapshot: (snapshot: RuntimeSessionSnapshot) => void
  markResyncing: (sessionId: string) => void
  markSyncFailed: (sessionId: string, message: string) => void
  setError: (sessionId: string, message: string | null) => void
  setPermissionMode: (sessionId: string, mode: RuntimePermissionMode) => void
  reconcileCanonical: (sessionId: string, preserveTerminal?: boolean) => void
  registerSubAgents: (parentSessionId: string, childSessionIds: string[]) => void
  clearSession: (sessionId: string) => void
}

function reduceRuntimeUpdates(
  previous: SessionRuntimeView,
  envelopes: RuntimeSessionUpdateEnvelope[],
  isSubAgent: boolean,
): { next: SessionRuntimeView; result: RuntimeApplyResult } {
  let next = previous
  let duplicate = false
  let sequenceGap = false
  let terminal = false
  let draftCleared = false

  for (const envelope of envelopes) {
    duplicate ||= envelope.sequence <= next.lastSequence
    sequenceGap ||= envelope.sequence > next.lastSequence + 1
    next = reduceSessionUpdate(next, envelope, isSubAgent)
    terminal ||= envelope.update.type === 'turn_finished'
    draftCleared ||= envelope.update.type === 'draft_cleared'
  }

  return { next, result: { duplicate, sequenceGap, terminal, draftCleared } }
}

function viewFor(state: RuntimeStoreState, sessionId: string): SessionRuntimeView {
  return state.bySession[sessionId] ?? createSessionRuntimeView()
}

export const useRuntimeStore = create<RuntimeStoreState>((set, get) => ({
  bySession: {},
  subAgentParentBySession: {},

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
    const { next, result } = reduceRuntimeUpdates(
      previous,
      [envelope],
      envelope.sessionId in get().subAgentParentBySession,
    )
    if (next !== previous) {
      set((state) => ({
        bySession: { ...state.bySession, [envelope.sessionId]: next },
      }))
    }
    return result
  },

  applyBatch: (envelopes) => {
    if (envelopes.length === 0) {
      return { duplicate: false, sequenceGap: false, terminal: false, draftCleared: false }
    }
    const sessionId = envelopes[0].sessionId
    if (envelopes.some((envelope) => envelope.sessionId !== sessionId)) {
      throw new Error('runtime update batch must contain exactly one session')
    }
    const previous = viewFor(get(), sessionId)
    const { next, result } = reduceRuntimeUpdates(
      previous,
      envelopes,
      sessionId in get().subAgentParentBySession,
    )
    if (next !== previous) {
      set((state) => ({
        bySession: { ...state.bySession, [sessionId]: next },
      }))
    }
    return result
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

  reconcileCanonical: (sessionId, preserveTerminal = false) => {
    set((state) => {
      const current = viewFor(state, sessionId)
      const terminalReconciled = current.phase === 'idle'
        && current.terminal !== null
        && !preserveTerminal
        && !(sessionId in state.subAgentParentBySession)
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

  registerSubAgents: (parentSessionId, childSessionIds) => {
    const currentChildren = new Set(childSessionIds)
    set((state) => {
      const subAgentParentBySession = { ...state.subAgentParentBySession }
      for (const [childSessionId, parent] of Object.entries(subAgentParentBySession)) {
        if (parent === parentSessionId && !currentChildren.has(childSessionId)) {
          delete subAgentParentBySession[childSessionId]
        }
      }
      for (const childSessionId of childSessionIds) {
        subAgentParentBySession[childSessionId] = parentSessionId
      }

      const bySession = { ...state.bySession }
      for (const childSessionId of childSessionIds) {
        const current = bySession[childSessionId]
        if (!current) continue
        bySession[childSessionId] = {
          ...current,
          assistantDraft: null,
          toolCalls: {},
          orderedToolCallIds: [],
          pendingPermission: null,
          plan: null,
        }
      }
      return { bySession, subAgentParentBySession }
    })
  },

  clearSession: (sessionId) => {
    set((state) => {
      const bySession = { ...state.bySession }
      const subAgentParentBySession = { ...state.subAgentParentBySession }
      const removedSessionIds = new Set([sessionId])
      for (const [childSessionId, parentSessionId] of Object.entries(subAgentParentBySession)) {
        if (childSessionId === sessionId || parentSessionId === sessionId) {
          removedSessionIds.add(childSessionId)
          delete subAgentParentBySession[childSessionId]
        }
      }
      for (const removedSessionId of removedSessionIds) delete bySession[removedSessionId]
      return { bySession, subAgentParentBySession }
    })
  },
}))

export const EMPTY_RUNTIME_VIEW = createSessionRuntimeView()
