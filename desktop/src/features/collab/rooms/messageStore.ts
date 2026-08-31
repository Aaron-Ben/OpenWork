import { create } from 'zustand'

import { collabCommands, type CollabMessage, type CollabRun } from '@/bridge/collab'
import { resolveErrorMessage } from '@/lib/commandError'

export interface MessageWindow {
  messages: CollabMessage[]
  runs: CollabRun[]
  loading: boolean
  error: string | null
}

interface MessageStoreState {
  byRoom: Record<string, MessageWindow>
  open: (roomId: string) => Promise<void>
  send: (roomId: string, body: string) => Promise<void>
}

const openVersions = new Map<string, number>()

export const useMessageStore = create<MessageStoreState>((set, get) => ({
  byRoom: {},
  open: async (roomId) => {
    const version = (openVersions.get(roomId) ?? 0) + 1
    openVersions.set(roomId, version)
    const current = get().byRoom[roomId] ?? { messages: [], runs: [], loading: false, error: null }
    set({
      byRoom: {
        ...get().byRoom,
        [roomId]: { ...current, loading: current.messages.length === 0, error: null },
      },
    })
    try {
      const [messages, runs] = await Promise.all([
        collabCommands.listMessages(roomId),
        collabCommands.listRuns(),
      ])
      if (openVersions.get(roomId) !== version) return
      set({ byRoom: { ...get().byRoom, [roomId]: { messages, runs, loading: false, error: null } } })
    } catch (error) {
      if (openVersions.get(roomId) !== version) return
      const latest = get().byRoom[roomId] ?? current
      set({ byRoom: { ...get().byRoom, [roomId]: { ...latest, loading: false, error: resolveErrorMessage(error) } } })
    }
  },
  send: async (roomId, body) => {
    await collabCommands.sendMessage(roomId, body)
    await get().open(roomId)
  },
}))
