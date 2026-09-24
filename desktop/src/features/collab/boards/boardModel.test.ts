import { describe, expect, it } from 'vitest'

import type { CollabBoard, CollabCard } from '@/bridge/collab'
import { beforeCardIdAt, boardSummary, cardFooter, dropIndexAt, isNoopMove } from './boardModel'

function card(id: string, overrides: Partial<CollabCard> = {}): CollabCard {
  return {
    id, boardId: 'board-1', columnId: 'col-1', title: id, description: null, position: 0,
    assigneeId: null, createdBy: 'local-user', ...overrides,
  }
}

const cards = [card('a'), card('b'), card('c')]

describe('beforeCardIdAt', () => {
  it('names the card the dragged one lands in front of, skipping the dragged card itself', () => {
    expect(beforeCardIdAt(cards, 'x', 0)).toBe('a')
    expect(beforeCardIdAt(cards, 'x', 3)).toBeNull()
    expect(beforeCardIdAt(cards, 'b', 1)).toBe('c')
    expect(beforeCardIdAt(cards, 'b', 2)).toBe('c')
    expect(beforeCardIdAt(cards, 'c', 2)).toBeNull()
  })
})

describe('isNoopMove', () => {
  it('detects drops that leave the card where it is', () => {
    expect(isNoopMove(cards, cards[1]!, 'col-1', 'c')).toBe(true)
    expect(isNoopMove(cards, cards[2]!, 'col-1', null)).toBe(true)
    expect(isNoopMove(cards, cards[1]!, 'col-1', 'a')).toBe(false)
    expect(isNoopMove(cards, cards[1]!, 'col-2', 'c')).toBe(false)
  })
})

describe('boardSummary', () => {
  it('counts cards and the Agents working on them', () => {
    const board: CollabBoard = {
      id: 'board-1', title: 'Release', description: null, createdBy: 'local-user',
      columns: [
        { id: 'col-1', title: 'Todo', position: 0, kind: 'todo', cards: [card('a', { assigneeId: 'ada', agentState: 'queued' })] },
        { id: 'col-2', title: 'Doing', position: 1, kind: 'doing', cards: [
          card('b', { assigneeId: 'ada', agentState: 'working' }),
          card('c', { assigneeId: 'ada', agentState: 'working' }),
          card('d', { assigneeId: 'bo', agentState: 'working' }),
        ] },
      ],
    }
    expect(boardSummary(board)).toEqual({ cardCount: 4, workingAgentCount: 2 })
  })
})

describe('cardFooter', () => {
  it('shows work in progress, a queue, no assignee, or when it was last updated', () => {
    expect(cardFooter(card('a', { assigneeId: 'ada', agentState: 'working' }))).toBe('working')
    expect(cardFooter(card('a', { assigneeId: 'ada', agentState: 'queued' }))).toBe('queued')
    expect(cardFooter(card('a'))).toBe('unassigned')
    expect(cardFooter(card('a', { assigneeId: 'ada' }))).toBe('updated')
  })
})

describe('dropIndexAt', () => {
  it('counts the cards whose middle is above the pointer', () => {
    expect(dropIndexAt([20, 60, 100], 10)).toBe(0)
    expect(dropIndexAt([20, 60, 100], 61)).toBe(2)
    expect(dropIndexAt([20, 60, 100], 500)).toBe(3)
    expect(dropIndexAt([], 50)).toBe(0)
  })
})
