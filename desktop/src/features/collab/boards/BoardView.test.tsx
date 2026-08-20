import { renderToStaticMarkup } from 'react-dom/server'
import { describe, expect, it } from 'vitest'

import type { CollabBoard, CollabRoomSummary } from '@/bridge/collab'
import { BoardCard } from './BoardView'

const room: CollabRoomSummary = {
  id: 'general', kind: 'group', title: 'General', nextSequence: 0,
  lastReadSequence: 0, unreadCount: 0, muted: false,
  members: [
    { id: 'user', displayName: 'You', kind: 'user', enabled: true, muted: false },
    { id: 'alice', displayName: 'Alice', kind: 'agent', enabled: true, muted: false },
  ],
}

const board: CollabBoard = {
  id: 'work', roomId: 'general', title: 'Shared work',
  createdAt: '2026-08-20T10:00:00+08:00', updatedAt: '2026-08-20T10:00:00+08:00',
  columns: [{
    id: 'done', boardId: 'work', title: 'Archive', position: 0, isDone: true,
    cards: [{
      id: 'card_1', boardId: 'work', columnId: 'done', title: 'Ship P4', description: null,
      position: 0, assigneeId: 'alice', claimedBy: 'alice',
      claimedAt: '2026-08-20T10:01:00+08:00', createdAt: '2026-08-20T10:00:00+08:00',
      updatedAt: '2026-08-20T10:01:00+08:00',
    }],
  }],
}

describe('BoardView', () => {
  it('shows assignment and the current claimant', () => {
    const html = renderToStaticMarkup(
      <BoardCard
        card={board.columns[0]!.cards[0]!}
        people={new Map(room.members.map((member) => [member.id, member.displayName]))}
        canMoveLeft={false}
        canMoveRight={false}
        onMoveLeft={() => undefined}
        onMoveRight={() => undefined}
        onRelease={() => undefined}
      />,
    )
    expect(html).toContain('data-card-claimant="alice"')
    expect(html).toContain('Alice')
  })
})
