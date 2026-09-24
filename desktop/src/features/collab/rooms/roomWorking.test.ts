import { describe, expect, it } from 'vitest'

import type { CollabAgent, CollabAgentActivity } from '@/bridge/collab'
import { roomWorkingLabel } from './roomWorking'

function agent(id: string, activity: CollabAgentActivity): CollabAgent {
  return {
    id,
    displayName: id.toUpperCase(),
    role: null,
    persona: '',
    engineId: 'opencode',
    mainModelId: 'deepseek/deepseek-flash',
    triageModelId: 'deepseek/deepseek-flash',
    configRevision: 1,
    agendaEnabled: false,
    archivedAt: null,
    activity,
  }
}

function working(roomId: string | null, startedAt: string): CollabAgentActivity {
  return { kind: 'working', roomId, roomTitle: null, cardId: null, cardTitle: null, startedAt }
}

const idle: CollabAgentActivity = { kind: 'idle', roomId: 'room-1', roomTitle: null, lastSpokeAt: null }

describe('roomWorkingLabel', () => {
  it('names the only Agent working in this room', () => {
    expect(roomWorkingLabel([
      agent('ada', working('room-1', '2026-09-25T10:00:00+08:00')),
      agent('bo', idle),
      agent('cy', working('room-2', '2026-09-25T10:00:00+08:00')),
    ], 'room-1')).toEqual({ key: 'collab.rooms.workingOne', values: { name: 'ADA' } })
  })

  it('names two Agents in the order they started', () => {
    expect(roomWorkingLabel([
      agent('bo', working('room-1', '2026-09-25T10:05:00+08:00')),
      agent('ada', working('room-1', '2026-09-25T02:01:00Z')),
    ], 'room-1')).toEqual({ key: 'collab.rooms.workingTwo', values: { first: 'ADA', second: 'BO' } })
  })

  it('names the first Agent and counts everyone when more are working', () => {
    expect(roomWorkingLabel([
      agent('ada', working('room-1', '2026-09-25T10:01:00+08:00')),
      agent('bo', working('room-1', '2026-09-25T10:02:00+08:00')),
      agent('cy', working('room-1', '2026-09-25T10:00:00+08:00')),
    ], 'room-1')).toEqual({ key: 'collab.rooms.workingMany', values: { first: 'CY', count: 3, others: 2 } })
  })

  it('shows nothing when no Agent is working in this room', () => {
    expect(roomWorkingLabel([
      agent('ada', idle),
      agent('bo', working(null, '2026-09-25T10:00:00+08:00')),
      agent('cy', { kind: 'queued', cardCount: 1, firstCardTitle: 'Fix login' }),
    ], 'room-1')).toBeNull()
  })
})
