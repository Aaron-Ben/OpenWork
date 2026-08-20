import { create } from 'zustand'

import { collabCommands, type CollabLogEntry } from '@/bridge/collab'
import { resolveErrorMessage } from '@/lib/commandError'

interface LogStoreState {
  entries: CollabLogEntry[]
  roomId: string | null
  loading: boolean
  error: string | null
  fetch: (roomId: string | null) => Promise<void>
}

export const useLogStore = create<LogStoreState>((set) => ({
  entries: [],
  roomId: null,
  loading: false,
  error: null,
  fetch: async (roomId) => {
    set({ roomId, loading: true, error: null })
    try {
      const entries = await collabCommands.listLogs(roomId, 300)
      set({ entries, loading: false })
    } catch (error) {
      set({ error: resolveErrorMessage(error), loading: false })
    }
  },
}))
