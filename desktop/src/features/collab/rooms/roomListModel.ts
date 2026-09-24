import type { CollabAgent, CollabRoomSummary } from '@/bridge/collab'

/** 房间列表的筛选（collaboration-desktop.md §7.1）；`direct` 是用户与某个 Agent 的私聊。 */
export type RoomFilter = 'all' | 'unread' | 'direct' | 'group'

export interface RoomListSections {
  pinned: CollabRoomSummary[]
  others: CollabRoomSummary[]
}

function matchesFilter(room: CollabRoomSummary, filter: RoomFilter): boolean {
  switch (filter) {
    case 'all': return true
    case 'unread': return room.unreadCount > 0
    case 'direct': return room.kind === 'direct'
    case 'group': return room.kind === 'group'
    default: {
      const unreachable: never = filter
      return unreachable
    }
  }
}

function matchesQuery(room: CollabRoomSummary, names: Map<string, string>, query: string): boolean {
  if (!query) return true
  const haystack = [room.title ?? '', ...room.memberIds.flatMap((id) => [id, names.get(id) ?? ''])]
  return haystack.some((text) => text.toLowerCase().includes(query))
}

/**
 * 用户所在房间的平铺列表：置顶的在前，其余保持 Server 给的“最近消息”顺序。
 * Agent 之间的房间不在这里，见 `whisperRooms`。
 */
export function roomListSections(
  rooms: CollabRoomSummary[],
  agents: CollabAgent[],
  filter: RoomFilter,
  query: string,
): RoomListSections {
  const names = new Map(agents.map((agent) => [agent.id, agent.displayName]))
  const needle = query.trim().toLowerCase()
  const visible = rooms.filter((room) => room.userIsMember
    && matchesFilter(room, filter)
    && matchesQuery(room, names, needle))
  return {
    pinned: visible.filter((room) => room.pinned),
    others: visible.filter((room) => !room.pinned),
  }
}

/** “未读”筛选旁的数字：有未读消息的房间数。 */
export function unreadRoomCount(rooms: CollabRoomSummary[]): number {
  return rooms.filter((room) => room.userIsMember && room.unreadCount > 0).length
}

/** 导航上的未读总数：用户所在房间的未读消息数之和。 */
export function totalUnread(rooms: CollabRoomSummary[]): number {
  return rooms
    .filter((room) => room.userIsMember)
    .reduce((sum, room) => sum + room.unreadCount, 0)
}

/** Agent 之间的私聊（collaboration-desktop.md §7.6）。 */
export function whisperRooms(rooms: CollabRoomSummary[]): CollabRoomSummary[] {
  return rooms.filter((room) => !room.userIsMember && room.kind === 'direct')
}
