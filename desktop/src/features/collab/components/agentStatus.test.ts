import { describe, expect, it } from 'vitest'

import type { CollabAgentActivity } from '@/bridge/collab'
import { agentStatusLine, agentStatusTag, elapsedText } from './agentStatus'

const working = (card: string | null, room: string | null): CollabAgentActivity => ({
  kind: 'working', roomId: room ? 'room-1' : null, roomTitle: room, cardId: card ? 'card-1' : null,
  cardTitle: card, startedAt: '2026-09-25T10:00:00+08:00',
})

describe('agentStatusTag', () => {
  it('maps each activity to its label and tone', () => {
    expect(agentStatusTag(working(null, null))).toEqual({ key: 'collab.status.tag.working', tone: 'success' })
    expect(agentStatusTag({ kind: 'queued', cardCount: 1, firstCardTitle: 'Fix' })).toEqual({ key: 'collab.status.tag.queued', tone: 'clay' })
    expect(agentStatusTag({ kind: 'error', message: 'x' })).toEqual({ key: 'collab.status.tag.error', tone: 'danger' })
    expect(agentStatusTag({ kind: 'idle', roomId: null, roomTitle: null, lastSpokeAt: null })).toEqual({ key: 'collab.status.tag.idle', tone: 'neutral' })
    expect(agentStatusTag({ kind: 'archived' })).toEqual({ key: 'collab.status.tag.archived', tone: 'neutral' })
  })
})

describe('agentStatusLine', () => {
  it('prefers the card, then the room, for work in progress', () => {
    expect(agentStatusLine(working('Backfill', 'Release'))).toEqual({
      key: 'collab.status.line.workingCard', values: { title: 'Backfill' }, since: '2026-09-25T10:00:00+08:00',
    })
    expect(agentStatusLine(working(null, 'Release'))).toEqual({
      key: 'collab.status.line.workingRoom', values: { room: 'Release' }, since: '2026-09-25T10:00:00+08:00',
    })
    expect(agentStatusLine(working(null, null))).toEqual({
      key: 'collab.status.line.working', values: {}, since: '2026-09-25T10:00:00+08:00',
    })
  })

  it('describes queued cards, errors and when an idle Agent last spoke', () => {
    expect(agentStatusLine({ kind: 'queued', cardCount: 2, firstCardTitle: 'Fix' })).toEqual({
      key: 'collab.status.line.queued', values: { count: 2, title: 'Fix' },
    })
    expect(agentStatusLine({ kind: 'error', message: 'Engine missing' })).toEqual({
      key: 'collab.status.line.error', values: { message: 'Engine missing' },
    })
    expect(agentStatusLine({ kind: 'idle', roomId: 'r', roomTitle: 'Release', lastSpokeAt: '2026-09-25T10:07:00+08:00' })).toEqual({
      key: 'collab.status.line.idleSpoke', values: { room: 'Release' }, at: '2026-09-25T10:07:00+08:00',
    })
    expect(agentStatusLine({ kind: 'idle', roomId: null, roomTitle: null, lastSpokeAt: null })).toEqual({
      key: 'collab.status.line.idle', values: {},
    })
  })
})

describe('elapsedText', () => {
  it('uses seconds, then minutes and seconds, then hours and minutes', () => {
    expect(elapsedText(42)).toEqual({ key: 'collab.elapsed.seconds', values: { seconds: 42 } })
    expect(elapsedText(192)).toEqual({ key: 'collab.elapsed.minutes', values: { minutes: 3, seconds: 12 } })
    expect(elapsedText(3_780)).toEqual({ key: 'collab.elapsed.hours', values: { hours: 1, minutes: 3 } })
  })
})
