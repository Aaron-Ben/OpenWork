import { useTranslation } from 'react-i18next'

import type { CollabAgent, CollabRoomSummary } from '@/bridge/collab'
import { AgentStatusTag, AgentStatusText } from '@/features/collab/components/AgentStatusBadge'
import { ParticipantAvatar } from '@/features/collab/components/ParticipantAvatar'
import { useNow } from '@/features/collab/components/useNow'
import { ManageMembersDialog } from './ManageMembersDialog'
import { useRoomViewStore } from './roomViewStore'

/** 成员状态里的已用时间每秒刷新。 */
const TICK_MS = 1_000

/** 房间信息（collaboration-desktop.md §7.5）：Agent 成员与当前状态、群组的成员管理、关注提示。 */
export function RoomInfoPanel({ room, members, agents }: {
  room: CollabRoomSummary
  members: CollabAgent[]
  agents: CollabAgent[]
}) {
  const { t } = useTranslation()
  const now = useNow(TICK_MS)
  const showAgent = useRoomViewStore((state) => state.showAgent)
  const managing = useRoomViewStore((state) => state.managing)
  const setManaging = useRoomViewStore((state) => state.setManaging)
  return (
    <>
      <div className="flex h-14 shrink-0 items-center border-b border-line px-4">
        <h3 className="text-sm font-semibold">{t('collab.rooms.roomInfo')}</h3>
      </div>
      <div className="flex items-center justify-between px-3.5 pb-1.5 pt-3.5">
        <span className="text-[11px] font-semibold tracking-wide text-ink-soft">{t('collab.rooms.members', { count: room.memberIds.length })}</span>
        {room.kind === 'group' ? (
          <button type="button" className="text-xs font-semibold text-clay-ink" onClick={() => setManaging(true)}>{t('collab.rooms.manage')}</button>
        ) : null}
      </div>
      <div className="flex flex-col gap-0.5 px-2">
        {members.map((agent) => (
          <div key={agent.id} className="flex items-center gap-2.5 rounded-lg px-2 py-1.5">
            <button type="button" aria-label={t('collab.rooms.viewProfile', { name: agent.displayName })} className="rounded-full" onClick={() => showAgent(agent.id)}>
              <ParticipantAvatar participantId={agent.id} name={agent.displayName} size={30} ring={agent.activity.kind === 'working'} />
            </button>
            <span className="flex min-w-0 flex-1 flex-col">
              <span className="truncate text-[13px] font-semibold">{agent.displayName} <span className="font-normal text-ink-soft">@{agent.id}</span></span>
              <span className="truncate text-xs text-ink-soft"><AgentStatusText activity={agent.activity} now={now} /></span>
            </span>
            <AgentStatusTag activity={agent.activity} />
          </div>
        ))}
      </div>
      <p className="mx-3.5 mb-4 mt-auto rounded-xl border border-line bg-paper p-3 text-xs leading-relaxed text-ink-soft">{t('collab.rooms.attentionHint')}</p>
      {managing ? <ManageMembersDialog roomId={room.id} agents={agents} onClose={() => setManaging(false)} /> : null}
    </>
  )
}
