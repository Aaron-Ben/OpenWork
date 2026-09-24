import { beforeEach, describe, expect, it } from 'vitest'

import type { CollabRoomMessage } from '@/bridge/collab'
import { useRoomViewStore } from './roomViewStore'

const quoted: CollabRoomMessage = {
  id: 'msg-1', roomId: 'room-1', sequence: 1, authorId: 'ada', body: 'Draft', quoted: null,
  authorName: 'Ada', authorKind: 'agent', authorRole: null, createdAt: '2026-09-25T10:00:00+08:00',
}

describe('useRoomViewStore', () => {
  beforeEach(() => useRoomViewStore.getState().reset())

  it('switches the side panel between room info, a card and an Agent, and closes back to info', () => {
    const view = useRoomViewStore.getState()
    view.showCard('card-a')
    expect(useRoomViewStore.getState().panel).toEqual({ kind: 'card', cardId: 'card-a' })
    view.showAgent('bo')
    expect(useRoomViewStore.getState().panel).toEqual({ kind: 'agent', agentId: 'bo' })
    view.closePanel()
    expect(useRoomViewStore.getState().panel).toEqual({ kind: 'info' })
  })

  it('forgets the quote, highlight and panel when another room opens', () => {
    const view = useRoomViewStore.getState()
    view.quote(quoted)
    view.highlight('msg-1')
    view.showCard('card-a')
    view.reset()
    expect(useRoomViewStore.getState()).toMatchObject({
      panel: { kind: 'info' }, quoting: null, highlightedMessageId: null,
    })
  })
})
