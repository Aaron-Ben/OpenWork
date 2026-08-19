import { create } from 'zustand'

import { collabCommands, type CollabMessage, type CollabMessagePage } from '@/bridge/collab'
import { resolveErrorMessage } from '@/lib/commandError'

export interface MessageWindow extends CollabMessagePage {
  loading: boolean
  error: string | null
}

export function mergeMessageWindow(
  current: CollabMessage[],
  incoming: CollabMessage[],
): CollabMessage[] {
  const bySequence = new Map(current.map((message) => [message.sequence, message]))
  for (const message of incoming) bySequence.set(message.sequence, message)
  return [...bySequence.values()].sort((left, right) => left.sequence - right.sequence)
}

interface MessageStoreState {
  byRoom: Record<string, MessageWindow>
  open: (roomId: string) => Promise<void>
  loadOlder: (roomId: string) => Promise<void>
  loadNewer: (roomId: string) => Promise<void>
  refreshTail: (roomId: string) => Promise<void>
  send: (roomId: string, body: string) => Promise<void>
}

const emptyWindow: MessageWindow = {
  messages: [],
  hasOlder: false,
  hasNewer: false,
  loading: false,
  error: null,
}

export const useMessageStore = create<MessageStoreState>((set, get) => {
  async function load(
    roomId: string,
    anchor: { kind: 'around' | 'before' | 'after'; sequence: number } | null,
    replace: boolean,
  ) {
    const current = get().byRoom[roomId] ?? emptyWindow
    set({ byRoom: { ...get().byRoom, [roomId]: { ...current, loading: true, error: null } } })
    try {
      const page = await collabCommands.messagePage(roomId, anchor)
      const latest = get().byRoom[roomId] ?? emptyWindow
      set({
        byRoom: {
          ...get().byRoom,
          [roomId]: {
            messages: replace ? page.messages : mergeMessageWindow(latest.messages, page.messages),
            hasOlder: anchor?.kind === 'after' ? latest.hasOlder : page.hasOlder,
            hasNewer: anchor?.kind === 'before' ? latest.hasNewer : page.hasNewer,
            loading: false,
            error: null,
          },
        },
      })
    } catch (error) {
      const latest = get().byRoom[roomId] ?? emptyWindow
      set({
        byRoom: {
          ...get().byRoom,
          [roomId]: { ...latest, loading: false, error: resolveErrorMessage(error) },
        },
      })
    }
  }

  return {
    byRoom: {},
    open: (roomId) => load(roomId, null, true),
    loadOlder: async (roomId) => {
      const sequence = get().byRoom[roomId]?.messages[0]?.sequence
      if (sequence != null) await load(roomId, { kind: 'before', sequence }, false)
    },
    loadNewer: async (roomId) => {
      const messages = get().byRoom[roomId]?.messages ?? []
      const sequence = messages[messages.length - 1]?.sequence
      if (sequence != null) await load(roomId, { kind: 'after', sequence }, false)
    },
    refreshTail: async (roomId) => {
      const window = get().byRoom[roomId]
      if (!window) return
      const sequence = window.messages[window.messages.length - 1]?.sequence ?? 0
      await load(roomId, { kind: 'after', sequence }, false)
    },
    send: async (roomId, body) => {
      await collabCommands.sendMessage(roomId, body)
      await get().refreshTail(roomId)
    },
  }
})
