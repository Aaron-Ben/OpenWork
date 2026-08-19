import { describe, expect, it, vi } from 'vitest'

import type { CollabEvent } from '@/bridge/collab'
import { createCollabEventController } from './collabEventController'

type SimpleEventType = Extract<
  CollabEvent,
  { type: 'rooms_changed' | 'agents_changed' | 'permissions_changed' | 'engine_changed' }
>['type']

function event(sequence: number, type: SimpleEventType): CollabEvent {
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
      applyAgentActivity: vi.fn(),
      recordHeld: vi.fn(),
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
      applyAgentActivity: vi.fn(),
      recordHeld: vi.fn(),
    })
    await controller.process(event(2, 'agents_changed'))
    expect(refreshAll).toHaveBeenCalledOnce()
  })

  it('routes normalized activity and HELD feedback without refreshing messages', async () => {
    const applyAgentActivity = vi.fn()
    const recordHeld = vi.fn()
    const refreshRoomTail = vi.fn(async () => undefined)
    const controller = createCollabEventController({
      refreshAll: vi.fn(async () => undefined),
      refreshRooms: vi.fn(async () => undefined),
      refreshAgents: vi.fn(async () => undefined),
      refreshPermissions: vi.fn(async () => undefined),
      refreshRoomTail,
      applyAgentActivity,
      recordHeld,
    })

    await controller.process({
      version: 1,
      sequence: 1,
      type: 'agent_activity_changed',
      agentId: 'alice',
      activity: { kind: 'executing', detail: '$ cargo test' },
    })
    await controller.process({
      version: 1,
      sequence: 2,
      type: 'reply_held',
      agentId: 'bob',
      roomId: 'general',
      peerSequence: 12,
    })

    expect(applyAgentActivity).toHaveBeenCalledWith('alice', {
      kind: 'executing',
      detail: '$ cargo test',
    })
    expect(recordHeld).toHaveBeenCalledWith({
      agentId: 'bob',
      roomId: 'general',
      peerSequence: 12,
    })
    expect(refreshRoomTail).not.toHaveBeenCalled()
  })
})
