import { create } from 'zustand'

import { collabCommands, type CollabRuntimeStatus } from '@/bridge/collab'
import { resolveErrorMessage } from '@/lib/commandError'

interface CollabRuntimeStoreState {
  status: CollabRuntimeStatus | null
  loading: boolean
  error: string | null
  fetch: () => Promise<void>
}

export const useCollabRuntimeStore = create<CollabRuntimeStoreState>((set) => ({
  status: null,
  loading: false,
  error: null,
  fetch: async () => {
    set({ loading: true, error: null })
    try {
      set({ status: await collabCommands.status(), loading: false })
    } catch (error) {
      set({ error: resolveErrorMessage(error), loading: false })
    }
  },
}))
