import { describe, expect, it, vi } from 'vitest'

import type { CollabInvalidation } from '@/bridge/collabEvents'
import {
  refreshForInvalidation,
  type InvalidationContext,
  type InvalidationRefreshers,
} from './invalidationCoordinator'

const context: InvalidationContext = {
  view: 'rooms',
  activeRoomId: 'room-a',
  activeRoomKind: 'group',
}

function invalidation(
  kind: CollabInvalidation['kind'],
  subjectId: string | null = null,
): CollabInvalidation {
  return {
    id: 'event-a',
    kind,
    subjectId,
    revision: null,
    publishedAt: 1,
  }
}

function refreshers(): InvalidationRefreshers & Record<string, ReturnType<typeof vi.fn>> {
  return {
    runtime: vi.fn().mockResolvedValue(undefined),
    agents: vi.fn().mockResolvedValue(undefined),
    rooms: vi.fn().mockResolvedValue(undefined),
    roomMembers: vi.fn().mockResolvedValue(undefined),
    messages: vi.fn().mockResolvedValue(undefined),
    boards: vi.fn().mockResolvedValue(undefined),
  }
}

describe('collaboration invalidation coordinator', () => {
  it('refreshes the room ordering and only the affected open message window', async () => {
    const actions = refreshers()

    await refreshForInvalidation(invalidation('message', 'room-a'), context, actions)

    expect(actions.rooms).toHaveBeenCalledOnce()
    expect(actions.messages).toHaveBeenCalledWith('room-a')
  })

  it('does not load a background room message window', async () => {
    const actions = refreshers()

    await refreshForInvalidation(invalidation('message', 'room-b'), context, actions)

    expect(actions.rooms).toHaveBeenCalledOnce()
    expect(actions.messages).not.toHaveBeenCalled()
  })

  it('refreshes boards only while the board workspace is visible', async () => {
    const hidden = refreshers()
    await refreshForInvalidation(invalidation('board', 'board-a'), context, hidden)
    expect(hidden.boards).not.toHaveBeenCalled()

    const visible = refreshers()
    await refreshForInvalidation(
      invalidation('board', 'board-a'),
      { ...context, view: 'boards' },
      visible,
    )
    expect(visible.boards).toHaveBeenCalledOnce()
  })

  it('refreshes runtime and the active run projection after runner changes', async () => {
    const actions = refreshers()

    await refreshForInvalidation(invalidation('runner_status'), context, actions)

    expect(actions.runtime).toHaveBeenCalledOnce()
    expect(actions.messages).toHaveBeenCalledWith('room-a')
  })
})
