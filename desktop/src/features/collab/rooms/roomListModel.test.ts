import { describe, expect, it } from 'vitest'

import type { CollabAgent, CollabRoomSummary } from '@/bridge/collab'
import { roomListSections, totalUnread, unreadRoomCount, whisperRooms } from './roomListModel'

function room(id: string, overrides: Partial<CollabRoomSummary> = {}): CollabRoomSummary {
  return {
    id,
    kind: 'group',
    title: id,
    unreadCount: 0,
    lastMessage: null,
    lastMessageAt: null,
    userIsMember: true,
    memberIds: ['local-user'],
    pinned: false,
    ...overrides,
  }
}

function agent(id: string, displayName: string): CollabAgent {
  return {
    id, displayName, role: null, persona: '', engineId: 'opencode',
    mainModelId: 'm', triageModelId: 't', configRevision: 1, agendaEnabled: false, archivedAt: null,
    activity: { kind: 'idle', roomId: null, roomTitle: null, lastSpokeAt: null },
  }
}

const rooms = [
  room('release', { title: 'Release v0.9', pinned: true, memberIds: ['local-user', 'ada', 'bo'] }),
  room('arch', { title: 'Architecture', unreadCount: 3, memberIds: ['local-user', 'bo'] }),
  room('dm-bo', { kind: 'direct', title: 'Bo', unreadCount: 1, memberIds: ['local-user', 'bo'] }),
  room('dm-ada', { kind: 'direct', title: 'Ada', memberIds: ['local-user', 'ada'] }),
  room('whisper', { kind: 'direct', title: 'Ada', userIsMember: false, memberIds: ['ada', 'bo'], unreadCount: 4 }),
]
const agents = [agent('ada', 'Ada'), agent('bo', 'Bo')]
const ids = (list: CollabRoomSummary[]) => list.map((item) => item.id)

describe('roomListSections', () => {
  it('lists pinned rooms first and keeps Agent-only rooms out', () => {
    const sections = roomListSections(rooms, agents, 'all', '')
    expect(ids(sections.pinned)).toEqual(['release'])
    expect(ids(sections.others)).toEqual(['arch', 'dm-bo', 'dm-ada'])
  })

  it('filters by unread, direct and group', () => {
    expect(ids(roomListSections(rooms, agents, 'unread', '').others)).toEqual(['arch', 'dm-bo'])
    expect(ids(roomListSections(rooms, agents, 'direct', '').others)).toEqual(['dm-bo', 'dm-ada'])
    const groups = roomListSections(rooms, agents, 'group', '')
    expect([...ids(groups.pinned), ...ids(groups.others)]).toEqual(['release', 'arch'])
  })

  it('searches room titles and member names without case', () => {
    const byTitle = roomListSections(rooms, agents, 'all', 'arch')
    expect([...ids(byTitle.pinned), ...ids(byTitle.others)]).toEqual(['arch'])
    const byMember = roomListSections(rooms, agents, 'all', ' ADA ')
    expect([...ids(byMember.pinned), ...ids(byMember.others)]).toEqual(['release', 'dm-ada'])
  })
})

describe('unread counts', () => {
  it('counts only rooms the user belongs to', () => {
    expect(unreadRoomCount(rooms)).toBe(2)
    expect(totalUnread(rooms)).toBe(4)
  })
})

describe('whisperRooms', () => {
  it('keeps only rooms between Agents', () => {
    expect(ids(whisperRooms(rooms))).toEqual(['whisper'])
  })
})
