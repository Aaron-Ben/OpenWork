import { create } from 'zustand'

import { collabCommands, type CollabMessage } from '@/bridge/collab'
import { resolveErrorMessage } from '@/lib/commandError'

export interface MessageWindow {
  messages: CollabMessage[]
  loading: boolean
  error: string | null
}

interface MessageStoreState {
  byRoom: Record<string, MessageWindow>
  open: (roomId: string) => Promise<void>
  send: (roomId: string, body: string) => Promise<void>
}

export const useMessageStore = create<MessageStoreState>((set, get) => ({
  byRoom: {},
  open: async (roomId) => {
    const current = get().byRoom[roomId] ?? { messages: [], loading: false, error: null }
    set({ byRoom: { ...get().byRoom, [roomId]: { ...current, loading: true, error: null } } })
    try {
      const messages = await collabCommands.listMessages(roomId)
      set({ byRoom: { ...get().byRoom, [roomId]: { messages, loading: false, error: null } } })
    } catch (error) {
      set({ byRoom: { ...get().byRoom, [roomId]: { ...current, loading: false, error: resolveErrorMessage(error) } } })
    }
  },
  send: async (roomId, body) => {
    await collabCommands.sendMessage(roomId, body)
    await get().open(roomId)
  },
}))
