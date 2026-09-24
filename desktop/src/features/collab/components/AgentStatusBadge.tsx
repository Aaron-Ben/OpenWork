import { useTranslation } from 'react-i18next'

import type { CollabAgentActivity } from '@/bridge/collab'
import { elapsedSeconds, formatBeijingClock } from '@/lib/dateTime'
import { cn } from '@/lib/utils'
import { agentStatusLine, agentStatusTag, elapsedText, type StatusTone } from './agentStatus'

const TONE_CLASSES: Record<StatusTone, string> = {
  success: 'bg-status-success-soft text-status-success-ink',
  clay: 'bg-clay-soft text-ink',
  danger: 'bg-status-danger-soft text-status-danger-ink',
  neutral: 'bg-code-bg text-ink-soft',
}

/** 状态标签（collaboration-desktop.md §7.5）。 */
export function AgentStatusTag({ activity }: { activity: CollabAgentActivity }) {
  const { t } = useTranslation()
  const tag = agentStatusTag(activity)
  return (
    <span className={cn('shrink-0 whitespace-nowrap rounded-full px-2 py-0.5 text-[11px] font-semibold', TONE_CLASSES[tag.tone])}>
      {t(tag.key)}
    </span>
  )
}

/** 一句当前状态，工作中附已用时间，空闲附上次发言的时间。`now` 由调用方按需刷新。 */
export function AgentStatusText({ activity, now }: { activity: CollabAgentActivity, now: number }) {
  const { t } = useTranslation()
  const line = agentStatusLine(activity)
  const parts = [t(line.key, line.values)]
  if (line.since) {
    const elapsed = elapsedText(elapsedSeconds(line.since, now))
    parts.push(t(elapsed.key, elapsed.values))
  }
  if (line.at) parts.push(formatBeijingClock(line.at))
  return <>{parts.join(' · ')}</>
}
