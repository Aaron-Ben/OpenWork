import { beforeEach, describe, expect, it, vi } from 'vitest'

import { collabCommands, type CollabRoomSummary } from '@/bridge/collab'
import { totalUnread, useRoomStore } from './roomStore'

vi.mock('@/bridge/collab', () => ({ collabCommands: { listRooms: vi.fn(), markRead: vi.fn() } }))

function room(id: string, unreadCount: number, muted: boolean): CollabRoomSummary {
  return {
    id,
    kind: 'group',
    title: id,
    nextSequence: unreadCount,
    lastReadSequence: 0,
    unreadCount,
    muted,
    members: [],
  }
}

describe('totalUnread', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    useRoomStore.setState({ rooms: [], loading: false, error: null })
  })

  it('keeps per-room unread visible but excludes muted rooms from the global badge', () => {
    const rooms = [room('general', 3, false), room('quiet', 7, true)]
    expect(rooms[1].unreadCount).toBe(7)
    expect(totalUnread(rooms)).toBe(3)
  })

  it('reloads canonical unread after marking the human cursor', async () => {
    vi.mocked(collabCommands.markRead).mockResolvedValue(undefined)
    vi.mocked(collabCommands.listRooms).mockResolvedValue([room('general', 0, false)])
    await useRoomStore.getState().markRead('general', 3)
    expect(collabCommands.markRead).toHaveBeenCalledWith('general', 3)
    expect(useRoomStore.getState().rooms[0]?.unreadCount).toBe(0)
  })
})
