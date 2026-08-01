import { create } from 'zustand'

import { coreCommands } from '../../bridge/commands'
import type { ProviderConfig } from '../models/contracts'
import type {
  RuntimeSessionRecord,
  RuntimeStoredMessage,
} from '../../bridge/compat'
import { resolveErrorMessage } from '../../utils/commandError'
import { useRuntimeStore } from '../chat/runtimeStore'

interface CreateSessionInput {
  title: string
  workingDirectory: string
  provider: ProviderConfig
  modelId: string
}

type LoadState = 'idle' | 'loading' | 'loaded' | 'error'

interface SessionStoreState {
  summaries: Record<string, RuntimeSessionRecord>
  orderedSessionIds: string[]
  activeSessionId: string | null
  messagesBySession: Record<string, RuntimeStoredMessage[]>
  loadStateBySession: Record<string, LoadState>
  isLoading: boolean
  error: string | null
  fetchAll: () => Promise<void>
  create: (input: CreateSessionInput) => Promise<string | null>
  select: (sessionId: string) => Promise<void>
  reload: (sessionId: string) => Promise<boolean>
  rename: (sessionId: string, title: string) => Promise<void>
  remove: (sessionId: string) => Promise<void>
  clearSelection: () => void
}

function newSessionId(): string {
  return `sess-${crypto.randomUUID().split('-').join('')}`
}

function modelRecordId(providerId: string, modelId: string): string {
  return `model:${providerId}:${modelId}`
}

let reloadRequestSequence = 0
const latestReloadRequestBySession = new Map<string, number>()

export const useSessionStore = create<SessionStoreState>((set, get) => ({
  summaries: {},
  orderedSessionIds: [],
  activeSessionId: null,
  messagesBySession: {},
  loadStateBySession: {},
  isLoading: false,
  error: null,

  fetchAll: async () => {
    set({ isLoading: true, error: null })
    try {
      const sessions = await coreCommands.listSessions()
      set((state) => ({
        summaries: Object.fromEntries(sessions.map((session) => [session.id, session])),
        orderedSessionIds: sessions.map((session) => session.id),
        activeSessionId:
          state.activeSessionId && sessions.some((session) => session.id === state.activeSessionId)
            ? state.activeSessionId
            : sessions[0]?.id ?? null,
        isLoading: false,
      }))
      const active = get().activeSessionId
      if (active && !(active in get().messagesBySession)) await get().reload(active)
    } catch (error) {
      set({ isLoading: false, error: resolveErrorMessage(error) })
    }
  },

  create: async ({ title, workingDirectory, provider, modelId }) => {
    try {
      const session = await coreCommands.createSession({
        id: newSessionId(),
        title,
        workingDirectory,
        defaultModelId: modelRecordId(provider.id, modelId),
      })
      set((state) => ({
        summaries: { ...state.summaries, [session.id]: session },
        orderedSessionIds: [session.id, ...state.orderedSessionIds],
        activeSessionId: session.id,
        messagesBySession: { ...state.messagesBySession, [session.id]: [] },
        loadStateBySession: { ...state.loadStateBySession, [session.id]: 'loaded' },
        error: null,
      }))
      return session.id
    } catch (error) {
      set({ error: resolveErrorMessage(error) })
      return null
    }
  },

  select: async (sessionId) => {
    set({ activeSessionId: sessionId })
    if (!(sessionId in get().messagesBySession)) await get().reload(sessionId)
  },

  reload: async (sessionId) => {
    const requestSequence = ++reloadRequestSequence
    latestReloadRequestBySession.set(sessionId, requestSequence)
    set((state) => ({
      loadStateBySession: { ...state.loadStateBySession, [sessionId]: 'loading' },
    }))
    try {
      const loaded = await coreCommands.loadSession(sessionId)
      if (latestReloadRequestBySession.get(sessionId) !== requestSequence) return false
      set((state) => ({
        summaries: { ...state.summaries, [sessionId]: loaded.session },
        orderedSessionIds: state.orderedSessionIds.includes(sessionId)
          ? state.orderedSessionIds
          : [sessionId, ...state.orderedSessionIds],
        messagesBySession: { ...state.messagesBySession, [sessionId]: loaded.messages },
        loadStateBySession: { ...state.loadStateBySession, [sessionId]: 'loaded' },
        error: null,
      }))
      return true
    } catch (error) {
      if (latestReloadRequestBySession.get(sessionId) !== requestSequence) return false
      set((state) => ({
        loadStateBySession: { ...state.loadStateBySession, [sessionId]: 'error' },
        error: resolveErrorMessage(error),
      }))
      return false
    } finally {
      if (latestReloadRequestBySession.get(sessionId) === requestSequence) {
        latestReloadRequestBySession.delete(sessionId)
      }
    }
  },

  rename: async (sessionId, title) => {
    try {
      const session = await coreCommands.renameSession(sessionId, title)
      set((state) => ({
        summaries: { ...state.summaries, [sessionId]: session },
        error: null,
      }))
    } catch (error) {
      set({ error: resolveErrorMessage(error) })
    }
  },

  remove: async (sessionId) => {
    try {
      await coreCommands.deleteSession(sessionId)
      set((state) => {
        const summaries = { ...state.summaries }
        const messagesBySession = { ...state.messagesBySession }
        const loadStateBySession = { ...state.loadStateBySession }
        delete summaries[sessionId]
        delete messagesBySession[sessionId]
        delete loadStateBySession[sessionId]
        const orderedSessionIds = state.orderedSessionIds.filter((id) => id !== sessionId)
        return {
          summaries,
          orderedSessionIds,
          messagesBySession,
          loadStateBySession,
          activeSessionId:
            state.activeSessionId === sessionId
              ? orderedSessionIds[0] ?? null
              : state.activeSessionId,
          error: null,
        }
      })
      useRuntimeStore.getState().clearSession(sessionId)
    } catch (error) {
      set({ error: resolveErrorMessage(error) })
    }
  },

  clearSelection: () => set({ activeSessionId: null }),
}))

export function selectSessions(state: SessionStoreState): RuntimeSessionRecord[] {
  return state.orderedSessionIds.flatMap((id) => state.summaries[id] ? [state.summaries[id]] : [])
}
