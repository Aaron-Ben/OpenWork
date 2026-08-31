import { create } from 'zustand'

import { collabCommands, type CollabBoard, type CollabRun } from '@/bridge/collab'
import { resolveErrorMessage } from '@/lib/commandError'

interface BoardStoreState {
  boards: CollabBoard[]
  runs: CollabRun[]
  loading: boolean
  error: string | null
  fetchAll: () => Promise<void>
  createBoard: (title: string) => Promise<void>
}

export const useBoardStore = create<BoardStoreState>((set, get) => ({
  boards: [],
  runs: [],
  loading: false,
  error: null,
  fetchAll: async () => {
    set({ loading: true, error: null })
    try {
      const [boards, runs] = await Promise.all([
        collabCommands.listBoards(),
        collabCommands.listRuns(),
      ])
      set({ boards, runs, loading: false })
    } catch (error) {
      set({ error: resolveErrorMessage(error), loading: false })
    }
  },
  createBoard: async (title) => {
    await collabCommands.createBoard(title)
    await get().fetchAll()
  },
}))
