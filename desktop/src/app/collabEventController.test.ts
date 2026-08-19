import { describe, expect, it, vi } from 'vitest'

import type { CollabEvent } from '@/bridge/collab'
import { createCollabEventController } from './collabEventController'

function event(sequence: number, type: CollabEvent['type']): CollabEvent {
  return type === 'rooms_changed'
    ? { version: 1, sequence, type, roomId: 'general' }
    : { version: 1, sequence, type }
}

describe('collab event controller', () => {
  it('keeps unread and permission refreshes on separate paths', async () => {
    const refreshRooms = vi.fn(async () => undefined)
    const refreshPermissions = vi.fn(async () => undefined)
    const controller = createCollabEventController({
      refreshAll: vi.fn(async () => undefined),
      refreshRooms,
      refreshAgents: vi.fn(async () => undefined),
      refreshPermissions,
      refreshRoomTail: vi.fn(async () => undefined),
    })

    await controller.process(event(1, 'rooms_changed'))
    expect(refreshRooms).toHaveBeenCalledOnce()
    expect(refreshPermissions).not.toHaveBeenCalled()
    await controller.process(event(2, 'permissions_changed'))
    expect(refreshPermissions).toHaveBeenCalledOnce()
    expect(refreshRooms).toHaveBeenCalledOnce()
  })

  it('does a canonical refresh when a sequence gap appears', async () => {
    const refreshAll = vi.fn(async () => undefined)
    const controller = createCollabEventController({
      refreshAll,
      refreshRooms: vi.fn(async () => undefined),
      refreshAgents: vi.fn(async () => undefined),
      refreshPermissions: vi.fn(async () => undefined),
      refreshRoomTail: vi.fn(async () => undefined),
    })
    await controller.process(event(2, 'agents_changed'))
    expect(refreshAll).toHaveBeenCalledOnce()
  })
})
