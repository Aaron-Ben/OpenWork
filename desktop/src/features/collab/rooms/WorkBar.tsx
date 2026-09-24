import { useTranslation } from 'react-i18next'

import type { CollabAgent } from '@/bridge/collab'
import { elapsedText } from '@/features/collab/components/agentStatus'
import { ParticipantAvatar } from '@/features/collab/components/ParticipantAvatar'
import { useNow } from '@/features/collab/components/useNow'
import { elapsedSeconds } from '@/lib/dateTime'
import { cn } from '@/lib/utils'
import { roomWorkers } from './roomWorking'
import { WorkingDots } from './WorkingDots'

/** 已用时间每秒刷新。 */
const TICK_MS = 1_000

/**
 * 输入框上方的工作条（collaboration-desktop.md §7.4）：高度固定，没有人工作时透明而不收起，
 * 出现与消失都不让消息区跳动。
 */
export function WorkBar({ agents, roomId }: { agents: CollabAgent[], roomId: string }) {
  const { t } = useTranslation()
  const now = useNow(TICK_MS)
  const [first, ...rest] = roomWorkers(agents, roomId)
  const elapsed = first ? elapsedText(elapsedSeconds(first.startedAt, now)) : null
  const label = first && elapsed
    ? rest.length > 0
      ? t('collab.rooms.workBarMore', { name: first.agent.displayName, count: rest.length + 1, others: rest.length, elapsed: t(elapsed.key, elapsed.values) })
      : first.cardTitle
        ? t('collab.rooms.workBarCard', { name: first.agent.displayName, title: first.cardTitle, elapsed: t(elapsed.key, elapsed.values) })
        : t('collab.rooms.workBarRoom', { name: first.agent.displayName, elapsed: t(elapsed.key, elapsed.values) })
    : ''
  return (
    <div aria-live="polite" data-work-bar className={cn('flex h-[22px] shrink-0 items-center gap-2 text-xs text-ink-soft', !first && 'invisible')}>
      {first ? (
        <>
          <WorkingDots />
          <ParticipantAvatar name={first.agent.displayName} isUser={false} size={18} />
          <span className="truncate">{label}</span>
        </>
      ) : null}
    </div>
  )
}
