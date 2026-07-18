import { create } from 'zustand'

import { runtimeApi } from '../api/runtime'
import type { ChatItem } from '../type/chat'
import type { ContentBlock, ToolCallBlock } from '../type/parts'
import type { ProviderConfig } from '../type/providers'
import type {
  RuntimeLiveToolCall,
  RuntimeSessionRecord,
  RuntimeSessionSnapshot,
  RuntimeSessionUpdateEnvelope,
  RuntimeStoredMessage,
} from '../type/runtime'
import { resolveErrorMessage } from '../utils/commandError'
import { useApprovalStore } from './approvalStore'

interface CreateRuntimeSessionInput {
  title: string
  workingDirectory: string
  provider: ProviderConfig
  modelId: string
}

interface ActiveTurn {
  sessionId: string
  clientRequestId: string
  turnId: string | null
}

interface RuntimeSessionStoreState {
  sessions: RuntimeSessionRecord[]
  activeSessionId: string | null
  messagesBySession: Record<string, ChatItem[]>
  lastSequenceBySession: Record<string, number>
  activeTurn: ActiveTurn | null
  isLoading: boolean
  error: string | null

  fetchAll: () => Promise<void>
  create: (input: CreateRuntimeSessionInput) => Promise<string | null>
  select: (id: string) => Promise<void>
  reload: (id: string) => Promise<void>
  rename: (id: string, title: string) => Promise<void>
  remove: (id: string) => Promise<void>
  clearSelection: () => void
  startTurn: (sessionId: string, text: string) => Promise<void>
  cancelActiveTurn: () => Promise<void>
  ingestUpdate: (payload: RuntimeSessionUpdateEnvelope) => Promise<void>
  recoverSnapshot: (sessionId: string) => Promise<void>
}

function modelRecordId(providerId: string, modelId: string): string {
  return `model:${providerId}:${modelId}`
}

function sessionId(): string {
  return `sess-${crypto.randomUUID().split('-').join('')}`
}

function clientRequestId(): string {
  return `request-${crypto.randomUUID().split('-').join('')}`
}

function toChatItems(messages: RuntimeStoredMessage[]): ChatItem[] {
  return messages
    .filter(
      (message): message is RuntimeStoredMessage & { role: 'user' | 'assistant' | 'tool' } =>
        message.role === 'user' || message.role === 'assistant' || message.role === 'tool',
    )
    .map((message) => ({
      id: message.id,
      turnId: message.turnId ?? undefined,
      role: message.role,
      parts: message.content,
    }))
}

function liveItem(messages: ChatItem[], turnId: string): ChatItem {
  return messages.find((item) => item.id === `live-${turnId}`) ?? {
    id: `live-${turnId}`,
    turnId,
    role: 'assistant',
    parts: [],
    isStreaming: true,
    requestId: turnId,
  }
}

function updateLiveItem(
  messages: ChatItem[],
  turnId: string,
  update: (item: ChatItem) => ChatItem,
): ChatItem[] {
  const id = `live-${turnId}`
  const existing = liveItem(messages, turnId)
  const next = update(existing)
  return messages.some((item) => item.id === id)
    ? messages.map((item) => (item.id === id ? next : item))
    : [...messages, next]
}

function appendTextPart(parts: ContentBlock[], type: 'text' | 'thinking', delta: string): ContentBlock[] {
  const last = parts[parts.length - 1]
  if (type === 'text' && last?.type === 'text') {
    return [...parts.slice(0, -1), { ...last, text: last.text + delta }]
  }
  if (type === 'thinking' && last?.type === 'thinking') {
    return [...parts.slice(0, -1), { ...last, thinking: last.thinking + delta }]
  }
  return [...parts, type === 'text' ? { type, text: delta } : { type, thinking: delta }]
}

function toolCallPart(tool: RuntimeLiveToolCall): ToolCallBlock {
  return {
    type: 'tool_call',
    id: tool.providerCallId,
    name: tool.name,
    input: JSON.stringify(tool.input),
    state: 'submitted',
  }
}

export const useRuntimeSessionStore = create<RuntimeSessionStoreState>((set, get) => ({
  sessions: [],
  activeSessionId: null,
  messagesBySession: {},
  lastSequenceBySession: {},
  activeTurn: null,
  isLoading: false,
  error: null,

  fetchAll: async () => {
    set({ isLoading: true, error: null })
    try {
      const sessions = await runtimeApi.listSessions()
      set({ sessions, isLoading: false })
      if (!get().activeSessionId && sessions.length > 0) {
        await get().select(sessions[0].id)
      }
    } catch (error) {
      set({ isLoading: false, error: resolveErrorMessage(error) })
    }
  },

  create: async ({ title, workingDirectory, provider, modelId }) => {
    const id = modelRecordId(provider.id, modelId)
    try {
      const session = await runtimeApi.createSession({
        id: sessionId(),
        title,
        workingDirectory,
        defaultModelId: id,
      })
      set((state) => ({
        sessions: [session, ...state.sessions],
        activeSessionId: session.id,
        messagesBySession: { ...state.messagesBySession, [session.id]: [] },
      }))
      return session.id
    } catch (error) {
      set({ error: resolveErrorMessage(error) })
      return null
    }
  },

  select: async (id) => {
    set({ activeSessionId: id })
    if (!(id in get().messagesBySession)) await get().reload(id)
    await get().recoverSnapshot(id)
  },

  reload: async (id) => {
    try {
      const loaded = await runtimeApi.loadSession(id)
      set((state) => {
        const live = (state.messagesBySession[id] ?? []).filter((item) => item.isStreaming)
        return {
          messagesBySession: {
            ...state.messagesBySession,
            [id]: [...toChatItems(loaded.messages), ...live],
          },
        }
      })
    } catch (error) {
      set({ error: resolveErrorMessage(error) })
    }
  },

  rename: async (id, title) => {
    try {
      const session = await runtimeApi.renameSession(id, title)
      set((state) => ({
        sessions: state.sessions.map((item) => (item.id === id ? session : item)),
      }))
    } catch (error) {
      set({ error: resolveErrorMessage(error) })
    }
  },

  remove: async (id) => {
    try {
      await runtimeApi.deleteSession(id)
      set((state) => {
        const sessions = state.sessions.filter((session) => session.id !== id)
        const messagesBySession = { ...state.messagesBySession }
        delete messagesBySession[id]
        return {
          sessions,
          messagesBySession,
          activeSessionId: state.activeSessionId === id ? sessions[0]?.id ?? null : state.activeSessionId,
        }
      })
    } catch (error) {
      set({ error: resolveErrorMessage(error) })
    }
  },

  clearSelection: () => set({ activeSessionId: null }),

  startTurn: async (targetSessionId, text) => {
    if (get().activeTurn || !text.trim()) return
    const requestId = clientRequestId()
    set((state) => ({
      activeTurn: { sessionId: targetSessionId, clientRequestId: requestId, turnId: null },
      messagesBySession: {
        ...state.messagesBySession,
        [targetSessionId]: [
          ...(state.messagesBySession[targetSessionId] ?? []),
          {
            id: `optimistic-${requestId}`,
            role: 'user',
            parts: [{ type: 'text', text: text.trim() }],
          },
        ],
      },
    }))
    try {
      const accepted = await runtimeApi.startTurn(targetSessionId, requestId, text.trim())
      set((state) => ({
        activeTurn: state.activeTurn?.clientRequestId === requestId
          ? { ...state.activeTurn, turnId: accepted.turnId }
          : state.activeTurn,
        messagesBySession: {
          ...state.messagesBySession,
          [targetSessionId]: (state.messagesBySession[targetSessionId] ?? []).map((item) =>
            item.id === `optimistic-${requestId}` ? { ...item, turnId: accepted.turnId } : item,
          ),
        },
      }))
    } catch (error) {
      set((state) => ({
        activeTurn: null,
        error: resolveErrorMessage(error),
        messagesBySession: {
          ...state.messagesBySession,
          [targetSessionId]: (state.messagesBySession[targetSessionId] ?? []).filter(
            (item) => item.id !== `optimistic-${requestId}`,
          ),
        },
      }))
    }
  },

  cancelActiveTurn: async () => {
    const active = get().activeTurn
    if (!active?.turnId) return
    try {
      await runtimeApi.cancelTurn(active.sessionId, active.turnId)
    } catch (error) {
      set({ error: resolveErrorMessage(error) })
    }
  },

  ingestUpdate: async (payload) => {
    let lastSequence = get().lastSequenceBySession[payload.sessionId] ?? 0
    if (payload.sequence > lastSequence + 1) {
      try {
        const replay = await runtimeApi.replayUpdates(payload.sessionId, lastSequence)
        for (const recovered of replay.sort((left, right) => left.sequence - right.sequence)) {
          if (recovered.sequence > (get().lastSequenceBySession[payload.sessionId] ?? 0)) {
            applyEnvelope(set, get, recovered)
          }
        }
      } catch {
        await get().recoverSnapshot(payload.sessionId)
      }
      lastSequence = get().lastSequenceBySession[payload.sessionId] ?? 0
    }
    if (payload.sequence > lastSequence) applyEnvelope(set, get, payload)
  },

  recoverSnapshot: async (targetSessionId) => {
    try {
      const snapshot = await runtimeApi.snapshot(targetSessionId)
      applySnapshot(set, get, snapshot)
    } catch {
      // A session with missing model credentials can still be listed and read.
    }
  },
}))

type StoreSet = Parameters<typeof useRuntimeSessionStore.setState>[0] extends never
  ? never
  : typeof useRuntimeSessionStore.setState
type StoreGet = typeof useRuntimeSessionStore.getState

function applyEnvelope(
  set: StoreSet,
  get: StoreGet,
  payload: RuntimeSessionUpdateEnvelope,
): void {
  const { sessionId, turnId, update } = payload
  if (update.type === 'permission_requested') {
    useApprovalStore.getState().push({
      id: update.request.toolCallId,
      turnId,
      sessionId,
      toolRunId: update.request.toolCallId,
      toolName: update.request.toolName,
      input: update.request.input,
      reason: update.request.reason,
    })
  } else if (update.type === 'permission_resolved') {
    useApprovalStore.getState().remove(update.toolCallId)
  } else if (update.type === 'turn_finished') {
    useApprovalStore.getState().removeByTurn(turnId)
  }

  set((state) => {
    let messages = state.messagesBySession[sessionId] ?? []
    if (update.type === 'turn_started') {
      messages = messages.map((item) =>
        item.id === `optimistic-${update.clientRequestId}` ? { ...item, turnId } : item,
      )
    } else if (update.type === 'text_delta' || update.type === 'reasoning_delta') {
      messages = updateLiveItem(messages, turnId, (item) => ({
        ...item,
        parts: appendTextPart(
          item.parts,
          update.type === 'text_delta' ? 'text' : 'thinking',
          update.delta,
        ),
      }))
    } else if (update.type === 'tool_call_started') {
      messages = updateLiveItem(messages, turnId, (item) => ({
        ...item,
        parts: [...item.parts, toolCallPart(update.toolCall)],
      }))
    } else if (update.type === 'tool_call_finished') {
      const providerCallId = update.providerCallId
      messages = updateLiveItem(messages, turnId, (item) => ({
        ...item,
        parts: [
          ...item.parts.map((part) =>
            part.type === 'tool_call' && part.id === providerCallId
              ? { ...part, state: 'finished' as const }
              : part,
          ),
          {
            type: 'tool_result',
            id: providerCallId,
            name: update.toolName,
            output: [{ type: 'text', text: update.output }],
            state: update.isError ? 'error' : 'success',
          },
        ],
      }))
    } else if (update.type === 'draft_cleared') {
      messages = messages.filter((item) => item.id !== `live-${turnId}`)
    } else if (update.type === 'turn_finished') {
      messages = messages.map((item) =>
        item.id === `live-${turnId}` ? { ...item, isStreaming: false } : item,
      )
    }
    return {
      lastSequenceBySession: {
        ...state.lastSequenceBySession,
        [sessionId]: payload.sequence,
      },
      messagesBySession: { ...state.messagesBySession, [sessionId]: messages },
      activeTurn: update.type === 'turn_finished'
        ? null
        : state.activeTurn?.sessionId === sessionId
          ? { ...state.activeTurn, turnId }
          : state.activeTurn,
    }
  })

  if (
    update.type === 'draft_cleared'
    || update.type === 'tool_call_finished'
    || update.type === 'turn_finished'
  ) {
    void get().reload(sessionId)
  }
}

function applySnapshot(set: StoreSet, get: StoreGet, snapshot: RuntimeSessionSnapshot): void {
  const { sessionId, runtime } = snapshot
  set((state) => {
    let messages = state.messagesBySession[sessionId] ?? []
    if (runtime.state === 'running') {
      messages = messages.filter((item) => item.id !== `live-${runtime.turnId}`)
      const parts: ContentBlock[] = []
      if (runtime.draftReasoning) parts.push({ type: 'thinking', thinking: runtime.draftReasoning })
      if (runtime.draftText) parts.push({ type: 'text', text: runtime.draftText })
      parts.push(...runtime.toolCalls.map(toolCallPart))
      if (parts.length > 0) {
        messages.push({
          id: `live-${runtime.turnId}`,
          turnId: runtime.turnId,
          role: 'assistant',
          parts,
          isStreaming: true,
          requestId: runtime.clientRequestId,
        })
      }
    }
    return {
      lastSequenceBySession: {
        ...state.lastSequenceBySession,
        [sessionId]: snapshot.lastUpdateSequence,
      },
      messagesBySession: { ...state.messagesBySession, [sessionId]: messages },
      activeTurn: runtime.state === 'running'
        ? {
            sessionId,
            turnId: runtime.turnId,
            clientRequestId: runtime.clientRequestId,
          }
        : state.activeTurn?.sessionId === sessionId
          ? null
          : state.activeTurn,
    }
  })
  if (runtime.state === 'running' && runtime.pendingPermission) {
    useApprovalStore.getState().push({
      id: runtime.pendingPermission.toolCallId,
      turnId: runtime.turnId,
      sessionId,
      toolRunId: runtime.pendingPermission.toolCallId,
      toolName: runtime.pendingPermission.toolName,
      input: runtime.pendingPermission.input,
      reason: runtime.pendingPermission.reason,
    })
  }
  void get().reload(sessionId)
}

const EMPTY_MESSAGES: ChatItem[] = []

export function useActiveRuntimeMessages(): ChatItem[] {
  const activeSessionId = useRuntimeSessionStore((state) => state.activeSessionId)
  return useRuntimeSessionStore((state) =>
    activeSessionId ? state.messagesBySession[activeSessionId] ?? EMPTY_MESSAGES : EMPTY_MESSAGES,
  )
}
