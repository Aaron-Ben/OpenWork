import { create } from 'zustand'

import { collabCommands, type CollabBoard, type CollabCardInput } from '@/bridge/collab'
import { resolveErrorMessage } from '@/lib/commandError'

interface BoardStoreState {
  byRoom: Record<string, CollabBoard[]>
  loading: boolean
  error: string | null
  fetchRoom: (roomId: string) => Promise<void>
  createBoard: (roomId: string, title: string) => Promise<boolean>
  createColumn: (
    roomId: string,
    boardId: string,
    title: string,
    position: number,
    isDone: boolean,
  ) => Promise<boolean>
  createCard: (roomId: string, card: CollabCardInput) => Promise<void>
  move: (roomId: string, cardId: string, columnId: string, position: number) => Promise<void>
  releaseClaim: (roomId: string, cardId: string, claimedBy: string) => Promise<void>
}

const EMPTY_BOARDS: readonly CollabBoard[] = Object.freeze([])

function newEntityId(prefix: 'board' | 'column'): string {
  return `${prefix}_${crypto.randomUUID().split('-').join('')}`
}

export function selectRoomBoards(
  byRoom: Readonly<Record<string, CollabBoard[]>>,
  roomId: string | null,
): readonly CollabBoard[] {
  return roomId ? byRoom[roomId] ?? EMPTY_BOARDS : EMPTY_BOARDS
}

export const useBoardStore = create<BoardStoreState>((set, get) => ({
  byRoom: {},
  loading: false,
  error: null,
  fetchRoom: async (roomId) => {
    set({ loading: true, error: null })
    try {
      const boards = await collabCommands.listBoards(roomId)
      set({ byRoom: { ...get().byRoom, [roomId]: boards }, loading: false })
    } catch (error) {
      set({ error: resolveErrorMessage(error), loading: false })
    }
  },
  createBoard: async (roomId, title) => {
    set({ error: null })
    try {
      await collabCommands.createBoard(newEntityId('board'), roomId, title)
      await get().fetchRoom(roomId)
      return true
    } catch (error) {
      set({ error: resolveErrorMessage(error) })
      return false
    }
  },
  createColumn: async (roomId, boardId, title, position, isDone) => {
    set({ error: null })
    try {
      await collabCommands.createBoardColumn(
        newEntityId('column'),
        boardId,
        title,
        position,
        isDone,
      )
      await get().fetchRoom(roomId)
      return true
    } catch (error) {
      set({ error: resolveErrorMessage(error) })
      return false
    }
  },
  createCard: async (roomId, card) => {
    await collabCommands.createCard(card)
    await get().fetchRoom(roomId)
  },
  move: async (roomId, cardId, columnId, position) => {
    await collabCommands.moveCard(cardId, columnId, position)
    await get().fetchRoom(roomId)
  },
  releaseClaim: async (roomId, cardId, claimedBy) => {
    await collabCommands.releaseCardClaim(cardId, claimedBy)
    await get().fetchRoom(roomId)
  },
}))
