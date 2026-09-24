import { X } from 'lucide-react'
import { useTranslation } from 'react-i18next'

import type { CollabAgent } from '@/bridge/collab'
import { useCollabNavigationStore } from '@/features/collab/collabNavigationStore'
import { AgentStatusTag, AgentStatusText } from '@/features/collab/components/AgentStatusBadge'
import { ParticipantAvatar } from '@/features/collab/components/ParticipantAvatar'
import { useNow } from '@/features/collab/components/useNow'
import { useRoomStore } from './roomStore'
import { useRoomViewStore } from './roomViewStore'

/** 状态里的已用时间每秒刷新。 */
const TICK_MS = 1_000

/** Agent 资料（collaboration-desktop.md §7.5，对照 Cumora `InfoPane.tsx`）。 */
export function AgentProfilePanel({ agentId, agents }: { agentId: string, agents: CollabAgent[] }) {
  const { t } = useTranslation()
  const now = useNow(TICK_MS)
  const openDirect = useRoomStore((state) => state.openDirect)
  const selectRoom = useCollabNavigationStore((state) => state.selectRoom)
  const navigate = useCollabNavigationStore((state) => state.navigate)
  const closePanel = useRoomViewStore((state) => state.closePanel)
  const agent = agents.find((candidate) => candidate.id === agentId) ?? null

  async function message() {
    const room = await openDirect(agentId)
    if (room) selectRoom(room.id)
  }

  return (
    <>
      <div className="flex h-14 shrink-0 items-center gap-2 border-b border-line pl-4 pr-2.5">
        <span className="flex-1 text-xs text-ink-faint">{t('collab.rooms.agentProfile')}</span>
        <button type="button" aria-label={t('collab.rooms.closeProfile')} className="grid size-[30px] place-items-center rounded-lg text-ink-soft hover:bg-paper" onClick={closePanel}><X size={15} /></button>
      </div>
      {agent ? (
        <div className="flex flex-1 flex-col gap-4 overflow-y-auto px-4 py-5">
          <div className="flex flex-col items-start gap-2.5">
            <ParticipantAvatar name={agent.displayName} isUser={false} size={64} ring={agent.activity.kind === 'working'} />
            <div className="flex flex-col gap-0.5">
              <h3 className="font-serif text-2xl font-semibold">{agent.displayName} <span className="font-sans text-[13px] font-normal text-ink-faint">@{agent.id}</span></h3>
              {agent.role ? <span className="font-serif text-sm italic text-ink-soft">{agent.role}</span> : null}
            </div>
          </div>
          <div className="flex items-center gap-2.5 rounded-xl border border-line bg-paper px-3 py-2.5 text-xs text-ink-soft">
            <AgentStatusTag activity={agent.activity} />
            <span className="min-w-0 flex-1"><AgentStatusText activity={agent.activity} now={now} /></span>
          </div>
          <div className="flex gap-2">
            <button type="button" disabled={agent.archivedAt !== null} className="h-[34px] flex-1 rounded-lg bg-ink text-[13px] font-semibold text-paper disabled:opacity-40" onClick={() => void message()}>{t('collab.rooms.directMessage')}</button>
            <button type="button" className="h-[34px] rounded-lg border border-line-strong px-3 text-[13px]" onClick={() => navigate('agents')}>{t('collab.rooms.editInAgents')}</button>
          </div>
          <div className="flex flex-col gap-1.5">
            <span className="text-[11px] font-semibold tracking-wide text-ink-faint">{t('collab.rooms.persona')}</span>
            <p className="line-clamp-6 whitespace-pre-line text-[13px] leading-relaxed text-ink-soft">{agent.persona}</p>
          </div>
          <div className="grid grid-cols-[64px_minmax(0,1fr)] gap-x-2.5 gap-y-1.5 text-xs">
            <span className="text-ink-faint">{t('collab.rooms.mainModel')}</span><code className="truncate font-mono text-ink-soft">{agent.mainModelId}</code>
            <span className="text-ink-faint">{t('collab.rooms.triageModel')}</span><code className="truncate font-mono text-ink-soft">{agent.triageModelId}</code>
          </div>
        </div>
      ) : (
        <p className="p-4 text-sm text-ink-faint">{t('collab.rooms.unknownAgent')}</p>
      )}
    </>
  )
}
