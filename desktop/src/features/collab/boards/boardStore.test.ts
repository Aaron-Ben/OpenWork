import { beforeEach, describe, expect, it, vi } from 'vitest'

import { collabCommands, type CollabBoard, type CollabCardInput } from '@/bridge/collab'
import { selectRoomBoards, useBoardStore } from './boardStore'

vi.mock('@/bridge/collab', () => ({
  collabCommands: {
    listBoards: vi.fn(),
    createBoard: vi.fn(),
    createBoardColumn: vi.fn(),
    createCard: vi.fn(),
    moveCard: vi.fn(),
    releaseCardClaim: vi.fn(),
  },
}))

const board: CollabBoard = {
  id: 'work',
  roomId: 'general',
  title: 'Shared work',
  createdAt: '2026-08-20T10:00:00+08:00',
  updatedAt: '2026-08-20T10:00:00+08:00',
  columns: [
    { id: 'looks_done', boardId: 'work', title: 'Done someday', position: 0, isDone: false, cards: [] },
    { id: 'finished', boardId: 'work', title: 'Archive', position: 1, isDone: true, cards: [] },
  ],
}

describe('boardStore', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    useBoardStore.setState({ byRoom: {}, loading: false, error: null })
  })

  it('returns one stable empty snapshot before a room board is available', () => {
    expect(selectRoomBoards({}, null)).toBe(selectRoomBoards({}, null))
    expect(selectRoomBoards({}, 'general')).toBe(selectRoomBoards({}, 'general'))
  })

  it('keeps the daemon isDone flag instead of classifying column titles', async () => {
    vi.mocked(collabCommands.listBoards).mockResolvedValue([board])
    await useBoardStore.getState().fetchRoom('general')
    expect(useBoardStore.getState().byRoom.general?.[0]?.columns).toMatchObject([
      { id: 'looks_done', isDone: false },
      { id: 'finished', isDone: true },
    ])
  })

  it('creates the first board in an empty room and reloads canonical room state', async () => {
    vi.mocked(collabCommands.createBoard).mockResolvedValue({ ...board, columns: [] })
    vi.mocked(collabCommands.listBoards).mockResolvedValue([{ ...board, columns: [] }])

    const created = await useBoardStore.getState().createBoard('general', 'Shared work')

    expect(created).toBe(true)
    expect(collabCommands.createBoard).toHaveBeenCalledWith(
      expect.stringMatching(/^board_/),
      'general',
      'Shared work',
    )
    expect(collabCommands.listBoards).toHaveBeenCalledWith('general')
  })

  it('creates a column with the user-selected done meaning and reloads the room', async () => {
    vi.mocked(collabCommands.createBoardColumn).mockResolvedValue(board.columns[1]!)
    vi.mocked(collabCommands.listBoards).mockResolvedValue([board])

    const created = await useBoardStore.getState().createColumn(
      'general',
      'work',
      'Archive',
      1,
      true,
    )

    expect(created).toBe(true)
    expect(collabCommands.createBoardColumn).toHaveBeenCalledWith(
      expect.stringMatching(/^column_/),
      'work',
      'Archive',
      1,
      true,
    )
    expect(collabCommands.listBoards).toHaveBeenCalledWith('general')
  })

  it('creates an assigned card and reloads canonical room state', async () => {
    const card: CollabCardInput = {
      boardId: 'work',
      columnId: 'looks_done',
      title: 'Implement board view',
      description: null,
      position: 0,
      assigneeId: 'alice',
    }
    vi.mocked(collabCommands.createCard).mockResolvedValue({
      card: {
        id: 'card_1', ...card, claimedBy: null, claimedAt: null,
        createdAt: '2026-08-20T10:00:00+08:00', updatedAt: '2026-08-20T10:00:00+08:00',
      },
      message: {
        id: 'msg_1', roomId: 'general', sequence: 1, authorId: 'user', kind: 'system',
        body: 'created', systemPayload: { type: 'card_created', cardId: 'card_1' },
        createdAt: '2026-08-20T10:00:00+08:00',
      },
    })
    vi.mocked(collabCommands.listBoards).mockResolvedValue([board])

    await useBoardStore.getState().createCard('general', card)
    expect(collabCommands.createCard).toHaveBeenCalledWith(card)
    expect(collabCommands.listBoards).toHaveBeenCalledWith('general')
  })
})
