import { beforeEach, describe, expect, it, vi } from 'vitest'

import { collabCommands, type CollabRoomSummary } from '@/bridge/collab'
import { totalUnread, useRoomStore } from './roomStore'

vi.mock('@/bridge/collab', () => ({
  collabCommands: {
    listRooms: vi.fn(),
    markRead: vi.fn(),
    setMuted: vi.fn(),
    addMember: vi.fn(),
    removeMember: vi.fn(),
    createRoom: vi.fn(),
  },
}))

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

  it('writes both human room mute and per-Agent room mute through the daemon', async () => {
    vi.mocked(collabCommands.setMuted).mockResolvedValue(undefined)
    vi.mocked(collabCommands.listRooms).mockResolvedValue([room('general', 2, true)])
    await useRoomStore.getState().setMuted('general', 'user', true)
    await useRoomStore.getState().setMuted('general', 'alice', true)
    expect(collabCommands.setMuted).toHaveBeenNthCalledWith(1, 'general', 'user', true)
    expect(collabCommands.setMuted).toHaveBeenNthCalledWith(2, 'general', 'alice', true)
    expect(useRoomStore.getState().rooms[0]?.muted).toBe(true)
  })

  it('removes members through the daemon and reloads the canonical roster', async () => {
    vi.mocked(collabCommands.removeMember).mockResolvedValue(undefined)
    vi.mocked(collabCommands.listRooms).mockResolvedValue([room('general', 0, false)])
    await useRoomStore.getState().removeMember('general', 'alice')
    expect(collabCommands.removeMember).toHaveBeenCalledWith('general', 'alice')
  })
})
