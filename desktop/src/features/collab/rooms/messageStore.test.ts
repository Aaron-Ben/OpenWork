import { beforeEach, describe, expect, it, vi } from 'vitest'

import { collabCommands, type CollabMessage } from '@/bridge/collab'
import { mergeMessageWindow, useMessageStore } from './messageStore'

vi.mock('@/bridge/collab', () => ({ collabCommands: { messagePage: vi.fn(), sendMessage: vi.fn() } }))

function message(sequence: number): CollabMessage {
  return {
    id: `message-${sequence}`,
    roomId: 'general',
    sequence,
    authorId: 'user',
    kind: 'normal',
    body: `message ${sequence}`,
    systemPayload: null,
    createdAt: '2026-08-19T10:00:00+08:00',
  }
}

describe('mergeMessageWindow', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    useMessageStore.setState({ byRoom: {} })
  })

  it('deduplicates overlapping pages and preserves sequence order', () => {
    expect(mergeMessageWindow([message(2), message(3)], [message(1), message(2)]))
      .toEqual([message(1), message(2), message(3)])
  })

  it('opens through the daemon default anchor instead of loading the entire room', async () => {
    vi.mocked(collabCommands.messagePage).mockResolvedValue({
      messages: [message(20), message(21)],
      hasOlder: true,
      hasNewer: true,
    })
    await useMessageStore.getState().open('general')
    expect(collabCommands.messagePage).toHaveBeenCalledWith('general', null)
    expect(useMessageStore.getState().byRoom.general?.messages).toHaveLength(2)
  })
})
