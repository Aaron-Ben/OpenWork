import { create } from 'zustand'

import {
  collabCommands,
  type CollabAgent,
  type CollabBoard,
} from '@/bridge/collab'
import { resolveErrorMessage } from '@/lib/commandError'

interface BoardStoreState {
  boards: CollabBoard[]
  agents: CollabAgent[]
  loading: boolean
  error: string | null
  fetchAll: () => Promise<void>
  createBoard: (title: string, description: string | null) => Promise<void>
  updateBoard: (boardId: string, title: string, description: string | null) => Promise<void>
  deleteBoard: (boardId: string) => Promise<void>
  createColumn: (boardId: string, title: string, isTerminal: boolean) => Promise<void>
  updateColumn: (columnId: string, title: string, isTerminal: boolean) => Promise<void>
  moveColumn: (columnId: string, beforeColumnId: string | null) => Promise<void>
  deleteColumn: (columnId: string) => Promise<void>
  assignCard: (cardId: string, assigneeId: string | null) => Promise<void>
  deleteCard: (cardId: string) => Promise<void>
}

export const useBoardStore = create<BoardStoreState>((set, get) => ({
  boards: [],
  agents: [],
  loading: false,
  error: null,
  fetchAll: async () => {
    set({ loading: true, error: null })
    try {
      const [boards, agents] = await Promise.all([
        collabCommands.listBoards(),
        collabCommands.listAgents(),
      ])
      set({ boards, agents, loading: false })
    } catch (error) {
      set({ error: resolveErrorMessage(error), loading: false })
    }
  },
  createBoard: async (title, description) => mutate(set, get, () =>
    collabCommands.createBoard(title, description)),
  updateBoard: async (boardId, title, description) => mutate(set, get, () =>
    collabCommands.updateBoard(boardId, title, description)),
  deleteBoard: async (boardId) => mutate(set, get, () =>
    collabCommands.deleteBoard(boardId)),
  createColumn: async (boardId, title, isTerminal) => mutate(set, get, () =>
    collabCommands.createBoardColumn(boardId, title, isTerminal)),
  updateColumn: async (columnId, title, isTerminal) => mutate(set, get, () =>
    collabCommands.updateBoardColumn(columnId, title, isTerminal)),
  moveColumn: async (columnId, beforeColumnId) => mutate(set, get, () =>
    collabCommands.moveBoardColumn(columnId, beforeColumnId)),
  deleteColumn: async (columnId) => mutate(set, get, () =>
    collabCommands.deleteBoardColumn(columnId)),
  assignCard: async (cardId, assigneeId) => mutate(set, get, () =>
    collabCommands.assignCard(cardId, assigneeId)),
  deleteCard: async (cardId) => mutate(set, get, () =>
    collabCommands.deleteCard(cardId)),
}))

async function mutate(
  set: (state: Partial<BoardStoreState>) => void,
  get: () => BoardStoreState,
  operation: () => Promise<unknown>,
) {
  set({ error: null })
  try {
    await operation()
    await get().fetchAll()
  } catch (error) {
    set({ error: resolveErrorMessage(error) })
    throw error
  }
}
