import { create } from 'zustand'

import { sessionsApi } from '../api/sessions'
import { applyEvent } from '../utils/streamAccumulator'
import type { ChatStreamEventPayload } from '../type/providers'
import type { ChatItem } from '../type/chat'
import type { SessionInput, SessionMessage, SessionSummary } from '../type/session'

interface SessionStoreState {
  sessions: SessionSummary[]
  activeSessionId: string | null
  messagesBySession: Record<string, ChatItem[]>
  isLoading: boolean
  error: string | null

  fetchAll: () => Promise<void>
  create: (input: SessionInput) => Promise<string | null>
  select: (id: string) => Promise<void>
  remove: (id: string) => Promise<void>
  rename: (id: string, title: string) => Promise<void>
  reload: (id: string) => Promise<void>

  pushUserMessage: (sessionId: string, text: string) => void
  ensureStreamingItem: (sessionId: string, requestId: string, model?: string) => void
  applyStreamEvent: (sessionId: string, payload: ChatStreamEventPayload) => void
  finishStreaming: (sessionId: string, requestId: string) => void
}

function toChatItems(messages: SessionMessage[]): ChatItem[] {
  return messages
    .filter(
      (message): message is SessionMessage & { role: 'user' | 'assistant' | 'tool' } =>
        message.role === 'user' || message.role === 'assistant' || message.role === 'tool',
    )
    .map((message) => ({ id: message.id, role: message.role, parts: message.parts }))
}

export const useSessionStore = create<SessionStoreState>((set, get) => ({
  sessions: [],
  activeSessionId: null,
  messagesBySession: {},
  isLoading: false,
  error: null,

  fetchAll: async () => {
    set({ isLoading: true, error: null })
    try {
      const sessions = await sessionsApi.list()
      set({ sessions, isLoading: false })
      if (!get().activeSessionId && sessions.length > 0) {
        await get().select(sessions[0].id)
      }
    } catch (error) {
      set({ isLoading: false, error: resolveErrorMessage(error) })
    }
  },

  create: async (input) => {
    const session = await sessionsApi.create(input)
    const summary: SessionSummary = {
      id: session.id,
      title: session.title,
      providerId: session.providerId,
      model: session.model,
      updatedAt: session.updatedAt,
    }
    set((state) => ({
      sessions: [summary, ...state.sessions],
      activeSessionId: session.id,
      messagesBySession: { ...state.messagesBySession, [session.id]: [] },
    }))
    return session.id
  },

  select: async (id) => {
    set({ activeSessionId: id })
    if (!(id in get().messagesBySession)) {
      try {
        const result = await sessionsApi.load(id)
        set((state) => ({
          messagesBySession: { ...state.messagesBySession, [id]: toChatItems(result.messages) },
        }))
      } catch (error) {
        set({ error: resolveErrorMessage(error) })
      }
    }
  },

  remove: async (id) => {
    await sessionsApi.remove(id)
    set((state) => {
      const sessions = state.sessions.filter((session) => session.id !== id)
      const messagesBySession = { ...state.messagesBySession }
      delete messagesBySession[id]
      const activeSessionId =
        state.activeSessionId === id ? sessions[0]?.id ?? null : state.activeSessionId
      return { sessions, messagesBySession, activeSessionId }
    })
  },

  rename: async (id, title) => {
    const session = await sessionsApi.rename(id, title)
    set((state) => ({
      sessions: state.sessions.map((sessionItem) =>
        sessionItem.id === id ? { ...sessionItem, title: session.title } : sessionItem,
      ),
    }))
  },

  reload: async (id) => {
    try {
      const result = await sessionsApi.load(id)
      set((state) => ({
        messagesBySession: { ...state.messagesBySession, [id]: toChatItems(result.messages) },
      }))
    } catch (error) {
      set({ error: resolveErrorMessage(error) })
    }
  },

  pushUserMessage: (sessionId, text) => {
    set((state) => {
      const messages = state.messagesBySession[sessionId] ?? []
      const item: ChatItem = {
        id: `user-${crypto.randomUUID()}`,
        role: 'user',
        parts: [{ type: 'text', text }],
      }
      return {
        messagesBySession: { ...state.messagesBySession, [sessionId]: [...messages, item] },
      }
    })
  },

  ensureStreamingItem: (sessionId, requestId, model) => {
    set((state) => {
      const messages = state.messagesBySession[sessionId] ?? []
      if (messages.some((item) => item.id === requestId)) return state
      const item: ChatItem = {
        id: requestId,
        role: 'assistant',
        parts: [],
        isStreaming: true,
        model,
      }
      return {
        messagesBySession: { ...state.messagesBySession, [sessionId]: [...messages, item] },
      }
    })
  },

  applyStreamEvent: (sessionId, payload) => {
    set((state) => {
      const messages = state.messagesBySession[sessionId] ?? []
      const next = applyEvent(messages, payload, payload.requestId)
      if (next === messages) return state
      return {
        messagesBySession: { ...state.messagesBySession, [sessionId]: next },
      }
    })
  },

  finishStreaming: (sessionId, requestId) => {
    set((state) => {
      const messages = state.messagesBySession[sessionId] ?? []
      return {
        messagesBySession: {
          ...state.messagesBySession,
          [sessionId]: messages.map((item) =>
            item.id === requestId ? { ...item, isStreaming: false } : item,
          ),
        },
      }
    })
  },
}))

/// 稳定的空数组引用 —— 避免 selector 每次返回新 `[]` 导致 useSyncExternalStore 判定
/// snapshot 变化、触发无限 re-render(Maximum update depth exceeded / 白屏)。
const EMPTY_MESSAGES: ChatItem[] = []

export function useActiveSessionMessages(): ChatItem[] {
  const activeSessionId = useSessionStore((state) => state.activeSessionId)
  return useSessionStore((state) =>
    activeSessionId ? state.messagesBySession[activeSessionId] ?? EMPTY_MESSAGES : EMPTY_MESSAGES,
  )
}

function resolveErrorMessage(error: unknown): string {
  if (typeof error === 'string') return error
  if (error instanceof Error) return error.message
  return 'Unexpected error'
}
