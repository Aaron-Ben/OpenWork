import type { CollabAgent } from '@/bridge/collab'

export interface RoomWorker {
  agent: CollabAgent
  cardTitle: string | null
  startedAt: string
}

/**
 * 本房间正在工作的 Agent（`activity` 为工作中且房间是本房间），按开始时间从早到晚
 * （collaboration-desktop.md §7.1、§7.4）。
 */
export function roomWorkers(agents: CollabAgent[], roomId: string): RoomWorker[] {
  return agents
    .flatMap((agent): RoomWorker[] => agent.activity.kind === 'working' && agent.activity.roomId === roomId
      ? [{ agent, cardTitle: agent.activity.cardTitle, startedAt: agent.activity.startedAt }]
      : [])
    .sort((left, right) => Date.parse(left.startedAt) - Date.parse(right.startedAt))
}

export type RoomWorkingLabel =
  | { key: 'collab.rooms.workingOne', values: { name: string } }
  | { key: 'collab.rooms.workingTwo', values: { first: string, second: string } }
  | { key: 'collab.rooms.workingMany', values: { first: string, count: number, others: number } }

/** 房间列表里“正在处理”的提示：按开始时间先后命名；没有人时返回 `null`。 */
export function roomWorkingLabel(agents: CollabAgent[], roomId: string): RoomWorkingLabel | null {
  const working = roomWorkers(agents, roomId).map((worker) => worker.agent)
  const [first, second] = working
  if (!first) return null
  if (!second) return { key: 'collab.rooms.workingOne', values: { name: first.displayName } }
  if (working.length === 2) {
    return { key: 'collab.rooms.workingTwo', values: { first: first.displayName, second: second.displayName } }
  }
  return { key: 'collab.rooms.workingMany', values: { first: first.displayName, count: working.length, others: working.length - 1 } }
}
