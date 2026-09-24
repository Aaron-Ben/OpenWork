import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import type { CollabRoomMessage, CollabRoomSnapshot } from '@/bridge/collab'

const { markRoomViewed, openRoom, listRooms } = vi.hoisted(() => ({
  listRooms: vi.fn(async () => []),
  markRoomViewed: vi.fn(async (_roomId: string, upToSeq: number) => upToSeq),
  openRoom: vi.fn(async (roomId: string): Promise<CollabRoomSnapshot> => ({ roomId, messages: [], notes: [] })),
}))

vi.mock('@/bridge/collab', () => ({
  collabCommands: { markRoomViewed, openRoom, listRooms },
}))

import { useMessageStore } from './messageStore'

function message(sequence: number): CollabRoomMessage {
  return {
    id: `msg-${sequence}`, roomId: 'room-1', sequence, authorId: 'ada', body: 'hi', quoted: null,
    authorName: 'Ada', authorKind: 'agent', authorRole: null, createdAt: '2026-09-25T10:00:00+08:00',
  }
}

function snapshotWith(messages: CollabRoomMessage[]) {
  return async (roomId: string): Promise<CollabRoomSnapshot> => ({ roomId, messages, notes: [] })
}

function setForeground(foreground: boolean) {
  vi.stubGlobal('document', {
    visibilityState: foreground ? 'visible' : 'hidden',
    hasFocus: () => foreground,
  })
}

describe('useMessageStore.markViewed', () => {
  beforeEach(() => {
    markRoomViewed.mockClear()
    openRoom.mockImplementation(snapshotWith([message(1), message(4)]))
  })

  afterEach(() => {
    vi.unstubAllGlobals()
  })

  it('reports what the room already shows once the window comes back to the foreground', async () => {
    setForeground(false)
    await useMessageStore.getState().open('room-1')
    expect(markRoomViewed).not.toHaveBeenCalled()

    setForeground(true)
    listRooms.mockClear()
    await useMessageStore.getState().markViewed('room-1')
    expect(markRoomViewed).toHaveBeenCalledWith('room-1', 4)
    // 未读数由 Server 计算，上报后重新取房间列表。
    expect(listRooms).toHaveBeenCalledOnce()

    await useMessageStore.getState().markViewed('room-1')
    expect(markRoomViewed).toHaveBeenCalledTimes(1)
  })

  it('keeps retrying when the report fails', async () => {
    setForeground(true)
    openRoom.mockImplementation(snapshotWith([message(7)]))
    markRoomViewed.mockRejectedValueOnce(new Error('offline'))
    await useMessageStore.getState().open('room-2').catch(() => undefined)
    markRoomViewed.mockClear()
    markRoomViewed.mockRejectedValueOnce(new Error('offline'))
    await expect(useMessageStore.getState().markViewed('room-2')).resolves.toBeUndefined()
    await useMessageStore.getState().markViewed('room-2')
    expect(markRoomViewed).toHaveBeenLastCalledWith('room-2', 7)
    expect(markRoomViewed).toHaveBeenCalledTimes(2)
  })

  it('does nothing for a room that has not been opened', async () => {
    setForeground(true)
    await useMessageStore.getState().markViewed('room-unknown')
    expect(markRoomViewed).not.toHaveBeenCalled()
  })
})

describe('useMessageStore.open', () => {
  afterEach(() => {
    vi.unstubAllGlobals()
  })

  // collaboration-desktop.md §4.2：打开房间只取一次快照，不再读 Run 列表。
  it('keeps the room snapshot as returned by the Server', async () => {
    setForeground(false)
    openRoom.mockImplementation(snapshotWith([message(2)]))
    await useMessageStore.getState().open('room-3')
    expect(useMessageStore.getState().byRoom['room-3']).toEqual({
      snapshot: { roomId: 'room-3', messages: [message(2)], notes: [] },
      loading: false,
      error: null,
    })
  })
})
