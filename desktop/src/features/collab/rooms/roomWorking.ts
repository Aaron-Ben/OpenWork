import type { CollabAgent } from '@/bridge/collab'

export type RoomWorkingLabel =
  | { key: 'collab.rooms.workingOne', values: { name: string } }
  | { key: 'collab.rooms.workingTwo', values: { first: string, second: string } }
  | { key: 'collab.rooms.workingMany', values: { first: string, count: number, others: number } }

function startedAt(agent: CollabAgent): number {
  return agent.activity.kind === 'working' ? Date.parse(agent.activity.startedAt) : 0
}

/**
 * 房间里“正在处理”的提示（collaboration-desktop.md §7.1）：只看 `activity` 为工作中且房间是本房间的
 * Agent，按开始时间先后命名；没有人时返回 `null`。
 */
export function roomWorkingLabel(agents: CollabAgent[], roomId: string): RoomWorkingLabel | null {
  const working = agents
    .filter((agent) => agent.activity.kind === 'working' && agent.activity.roomId === roomId)
    .sort((left, right) => startedAt(left) - startedAt(right))
  const [first, second] = working
  if (!first) return null
  if (!second) return { key: 'collab.rooms.workingOne', values: { name: first.displayName } }
  if (working.length === 2) {
    return { key: 'collab.rooms.workingTwo', values: { first: first.displayName, second: second.displayName } }
  }
  return { key: 'collab.rooms.workingMany', values: { first: first.displayName, count: working.length, others: working.length - 1 } }
}
