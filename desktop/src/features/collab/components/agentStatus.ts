import type { CollabAgentActivity } from '@/bridge/collab'

/** 状态标签的色调（collaboration-desktop.md §7.5）。 */
export type StatusTone = 'success' | 'clay' | 'danger' | 'neutral'

export interface StatusTag {
  key: `collab.status.tag.${CollabAgentActivity['kind']}`
  tone: StatusTone
}

/** 一句当前状态；`since` 是开始工作的时间（显示已用时间），`at` 是上次发言的时间。 */
export interface StatusLine {
  key: `collab.status.line.${string}`
  values: Record<string, string | number>
  since?: string
  at?: string
}

/** Agent 的状态标签：工作中（success）、已唤醒（clay）、空闲（中性）、出错（danger）、已归档（中性）。 */
export function agentStatusTag(activity: CollabAgentActivity): StatusTag {
  switch (activity.kind) {
    case 'working': return { key: 'collab.status.tag.working', tone: 'success' }
    case 'queued': return { key: 'collab.status.tag.queued', tone: 'clay' }
    case 'error': return { key: 'collab.status.tag.error', tone: 'danger' }
    case 'idle': return { key: 'collab.status.tag.idle', tone: 'neutral' }
    case 'archived': return { key: 'collab.status.tag.archived', tone: 'neutral' }
    default: {
      const unreachable: never = activity
      return unreachable
    }
  }
}

/** Agent 的一句当前状态（collaboration-desktop.md §7.5、§8）。 */
export function agentStatusLine(activity: CollabAgentActivity): StatusLine {
  switch (activity.kind) {
    case 'working':
      if (activity.cardTitle) return { key: 'collab.status.line.workingCard', values: { title: activity.cardTitle }, since: activity.startedAt }
      if (activity.roomTitle) return { key: 'collab.status.line.workingRoom', values: { room: activity.roomTitle }, since: activity.startedAt }
      return { key: 'collab.status.line.working', values: {}, since: activity.startedAt }
    case 'queued':
      return { key: 'collab.status.line.queued', values: { count: activity.cardCount, title: activity.firstCardTitle } }
    case 'error':
      return { key: 'collab.status.line.error', values: { message: activity.message } }
    case 'idle':
      return activity.lastSpokeAt
        ? { key: 'collab.status.line.idleSpoke', values: { room: activity.roomTitle ?? '' }, at: activity.lastSpokeAt }
        : { key: 'collab.status.line.idle', values: {} }
    case 'archived':
      return { key: 'collab.status.line.archived', values: {} }
    default: {
      const unreachable: never = activity
      return unreachable
    }
  }
}

/** 已用时间的文案键：不到一分钟用秒，不到一小时用分秒，否则用时分。 */
export function elapsedText(seconds: number): { key: `collab.elapsed.${string}`, values: Record<string, number> } {
  if (seconds < 60) return { key: 'collab.elapsed.seconds', values: { seconds } }
  if (seconds < 3_600) return { key: 'collab.elapsed.minutes', values: { minutes: Math.floor(seconds / 60), seconds: seconds % 60 } }
  return { key: 'collab.elapsed.hours', values: { hours: Math.floor(seconds / 3_600), minutes: Math.floor((seconds % 3_600) / 60) } }
}
