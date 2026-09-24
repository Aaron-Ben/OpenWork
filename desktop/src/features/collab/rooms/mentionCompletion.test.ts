import { describe, expect, it } from 'vitest'

import type { CollabAgent } from '@/bridge/collab'
import { activeMention, insertMention, mentionCandidates } from './mentionCompletion'

function agent(id: string, displayName: string, role: string | null = null): CollabAgent {
  return {
    id, displayName, role, persona: '', engineId: 'opencode',
    mainModelId: 'm', triageModelId: 't', configRevision: 1, agendaEnabled: false, archivedAt: null,
    activity: { kind: 'idle', roomId: null, roomTitle: null, lastSpokeAt: null },
  }
}

describe('activeMention', () => {
  it('finds the @ word that ends at the caret', () => {
    expect(activeMention('hi @b', 5)).toEqual({ start: 3, query: 'b' })
    expect(activeMention('@', 1)).toEqual({ start: 0, query: '' })
    expect(activeMention('hi @bo there', 6)).toEqual({ start: 3, query: 'bo' })
  })

  it('ignores an @ inside a word or before whitespace', () => {
    expect(activeMention('me@bo', 5)).toBeNull()
    expect(activeMention('@bo there', 9)).toBeNull()
  })
})

describe('mentionCandidates', () => {
  const agents = [agent('ada', 'Ada', 'Architect'), agent('bo', 'Bo'), { ...agent('old', 'Old'), archivedAt: '2026-09-01T00:00:00+08:00' }]

  it('offers @all first, then active room Agents matching id or name', () => {
    expect(mentionCandidates('', agents).map((candidate) => candidate.id)).toEqual(['all', 'ada', 'bo'])
    expect(mentionCandidates('a', agents).map((candidate) => candidate.id)).toEqual(['all', 'ada'])
    expect(mentionCandidates('B', agents).map((candidate) => candidate.id)).toEqual(['bo'])
  })
})

describe('insertMention', () => {
  it('replaces the typed query and leaves the caret after a space', () => {
    expect(insertMention('hi @b rest', { start: 3, query: 'b' }, 'bo')).toEqual({ draft: 'hi @bo  rest', caret: 7 })
  })
})
