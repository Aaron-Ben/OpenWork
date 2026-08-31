import { create } from 'zustand'

import {
  collabCommands,
  type CollabBoard,
} from '@/bridge/collab'
import { resolveErrorMessage } from '@/lib/commandError'

interface BoardStoreState {
  boards: CollabBoard[]
  selectedBoardId: string | null
  loading: boolean
  error: string | null
  selectBoard: (boardId: string) => void
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

let fetchVersion = 0

export const useBoardStore = create<BoardStoreState>((set, get) => ({
  boards: [],
  selectedBoardId: null,
  loading: false,
  error: null,
  selectBoard: (selectedBoardId) => set({ selectedBoardId }),
  fetchAll: async () => {
    const version = ++fetchVersion
    set({ loading: true, error: null })
    try {
      const boards = await collabCommands.listBoards()
      if (version !== fetchVersion) return
      const selectedBoardId = boards.some((board) => board.id === get().selectedBoardId)
        ? get().selectedBoardId
        : boards[0]?.id ?? null
      set({ boards, selectedBoardId, loading: false })
    } catch (error) {
      if (version === fetchVersion) set({ error: resolveErrorMessage(error), loading: false })
    }
  },
  createBoard: async (title, description) => {
    set({ error: null })
    try {
      const board = await collabCommands.createBoard(title, description)
      await get().fetchAll()
      set({ selectedBoardId: board.id })
    } catch (error) {
      set({ error: resolveErrorMessage(error) })
      throw error
    }
  },
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
