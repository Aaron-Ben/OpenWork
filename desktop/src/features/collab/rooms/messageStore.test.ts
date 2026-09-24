import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import type { CollabMessage } from '@/bridge/collab'

const { markRoomViewed, listMessages } = vi.hoisted(() => ({
  markRoomViewed: vi.fn(async (_roomId: string, upToSeq: number) => upToSeq),
  listMessages: vi.fn(async (): Promise<CollabMessage[]> => []),
}))

vi.mock('@/bridge/collab', () => ({
  collabCommands: { markRoomViewed, listMessages },
}))

import { useMessageStore } from './messageStore'

function message(sequence: number): CollabMessage {
  return { id: `msg-${sequence}`, roomId: 'room-1', sequence, authorId: 'ada', body: 'hi', quoted: null }
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
    listMessages.mockResolvedValue([message(1), message(4)])
  })

  afterEach(() => {
    vi.unstubAllGlobals()
  })

  it('reports what the room already shows once the window comes back to the foreground', async () => {
    setForeground(false)
    await useMessageStore.getState().open('room-1')
    expect(markRoomViewed).not.toHaveBeenCalled()

    setForeground(true)
    await useMessageStore.getState().markViewed('room-1')
    expect(markRoomViewed).toHaveBeenCalledWith('room-1', 4)

    await useMessageStore.getState().markViewed('room-1')
    expect(markRoomViewed).toHaveBeenCalledTimes(1)
  })

  it('keeps retrying when the report fails', async () => {
    setForeground(true)
    listMessages.mockResolvedValue([message(7)])
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

  // collaboration-desktop.md §4.1：“正在处理”来自 Agent 的 activity，房间窗口不再读取 Run 列表。
  it('loads only the room messages', async () => {
    setForeground(false)
    listMessages.mockResolvedValue([message(2)])
    await useMessageStore.getState().open('room-3')
    expect(useMessageStore.getState().byRoom['room-3']).toEqual({
      messages: [message(2)],
      loading: false,
      error: null,
    })
  })
})
