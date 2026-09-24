import { create } from 'zustand'

import {
  collabCommands,
  type CollabBoard,
  type CollabCardChange,
  type CollabCardInput,
  type CollabColumnKind,
} from '@/bridge/collab'
import { resolveErrorMessage } from '@/lib/commandError'

interface BoardStoreState {
  boards: CollabBoard[]
  selectedBoardId: string | null
  /** 右侧详情里打开的卡片；从房间“打开看板”时也选中它（collaboration-desktop.md §7.5、§9）。 */
  selectedCardId: string | null
  loading: boolean
  error: string | null
  selectBoard: (boardId: string) => void
  focusCard: (boardId: string, cardId: string) => void
  selectCard: (cardId: string | null) => void
  fetchAll: () => Promise<void>
  createBoard: (title: string, description: string | null) => Promise<void>
  updateBoard: (boardId: string, title: string, description: string | null) => Promise<void>
  deleteBoard: (boardId: string) => Promise<void>
  createColumn: (boardId: string, title: string, kind: CollabColumnKind | null) => Promise<void>
  updateColumn: (columnId: string, title: string, kind: CollabColumnKind | null) => Promise<void>
  moveColumn: (columnId: string, beforeColumnId: string | null) => Promise<void>
  deleteColumn: (columnId: string) => Promise<void>
  createCard: (input: CollabCardInput) => Promise<CollabCardChange>
  updateCard: (cardId: string, title: string | null, description: string | null) => Promise<CollabCardChange>
  moveCard: (cardId: string, columnId: string, beforeCardId: string | null) => Promise<CollabCardChange>
  assignCard: (cardId: string, assigneeId: string | null) => Promise<CollabCardChange>
  deleteCard: (cardId: string) => Promise<void>
}

let fetchVersion = 0

export const useBoardStore = create<BoardStoreState>((set, get) => ({
  boards: [],
  selectedBoardId: null,
  selectedCardId: null,
  loading: false,
  error: null,
  selectBoard: (selectedBoardId) => set({ selectedBoardId, selectedCardId: null }),
  focusCard: (selectedBoardId, selectedCardId) => set({ selectedBoardId, selectedCardId }),
  selectCard: (selectedCardId) => set({ selectedCardId }),
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
  updateBoard: async (boardId, title, description) => settle(set, get, () =>
    collabCommands.updateBoard(boardId, title, description)),
  deleteBoard: async (boardId) => settle(set, get, () =>
    collabCommands.deleteBoard(boardId)),
  createColumn: async (boardId, title, kind) => settle(set, get, () =>
    collabCommands.createBoardColumn(boardId, title, kind)),
  updateColumn: async (columnId, title, kind) => settle(set, get, () =>
    collabCommands.updateBoardColumn(columnId, title, kind)),
  moveColumn: async (columnId, beforeColumnId) => settle(set, get, () =>
    collabCommands.moveBoardColumn(columnId, beforeColumnId)),
  deleteColumn: async (columnId) => settle(set, get, () =>
    collabCommands.deleteBoardColumn(columnId)),
  createCard: async (input) => mutate(set, get, () => collabCommands.createCard(input)),
  updateCard: async (cardId, title, description) => mutate(set, get, () =>
    collabCommands.updateCard(cardId, title, description)),
  moveCard: async (cardId, columnId, beforeCardId) => mutate(set, get, () =>
    collabCommands.moveCard(cardId, columnId, beforeCardId)),
  assignCard: async (cardId, assigneeId) => mutate(set, get, () =>
    collabCommands.assignCard(cardId, assigneeId)),
  deleteCard: async (cardId) => settle(set, get, () =>
    collabCommands.deleteCard(cardId)),
}))

/** 同 `mutate`，不需要结果时用。 */
async function settle(
  set: (state: Partial<BoardStoreState>) => void,
  get: () => BoardStoreState,
  operation: () => Promise<unknown>,
): Promise<void> {
  await mutate(set, get, operation)
}

/** 执行一次修改并重新取看板，界面以 Server 返回的结果为准；失败时记下错误并继续抛出。 */
async function mutate<T>(
  set: (state: Partial<BoardStoreState>) => void,
  get: () => BoardStoreState,
  operation: () => Promise<T>,
): Promise<T> {
  set({ error: null })
  try {
    const result = await operation()
    await get().fetchAll()
    return result
  } catch (error) {
    set({ error: resolveErrorMessage(error) })
    throw error
  }
}
