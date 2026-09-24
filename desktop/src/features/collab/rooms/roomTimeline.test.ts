import { describe, expect, it } from 'vitest'

import type { CollabBoard, CollabRoomMessage, CollabRoomNote } from '@/bridge/collab'
import { findCard, roomTimeline } from './roomTimeline'

function message(sequence: number): CollabRoomMessage {
  return {
    id: `msg-${sequence}`, roomId: 'room-1', sequence, authorId: 'ada', body: 'hi', quoted: null,
    authorName: 'Ada', authorKind: 'agent', authorRole: null, createdAt: '2026-09-25T10:00:00+08:00',
  }
}

describe('roomTimeline', () => {
  it('places each note right after the message it explains', () => {
    const notes: CollabRoomNote[] = [
      { kind: 'loop_cap', afterSequence: 2 },
      { kind: 'routing', afterSequence: 1, skippedNames: ['Ada'], targetNames: ['Bo'] },
    ]
    expect(roomTimeline([message(1), message(2), message(3)], notes).map((item) => item.key)).toEqual([
      'msg-1', 'note-routing-1', 'msg-2', 'note-loop_cap-2', 'msg-3',
    ])
  })
})

describe('findCard', () => {
  const board: CollabBoard = {
    id: 'board-1', title: 'Release', description: null, createdBy: 'local-user',
    columns: [{
      id: 'col-1', title: 'Doing', position: 0, kind: 'doing',
      cards: [{ id: 'card-a', boardId: 'board-1', columnId: 'col-1', title: 'Backfill', description: null, position: 0, assigneeId: 'ada', createdBy: 'ada' }],
    }],
  }

  it('returns the card with its board and column, or null when it is gone', () => {
    const found = findCard([board], 'card-a')
    expect([found?.card.title, found?.board.title, found?.column.title]).toEqual(['Backfill', 'Release', 'Doing'])
    expect(findCard([board], 'card-b')).toBeNull()
  })
})
