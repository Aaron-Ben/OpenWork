import { Archive, Bot, CalendarClock, MessageSquare, Pencil, RotateCcw } from 'lucide-react'
import { useState } from 'react'
import { useTranslation } from 'react-i18next'

import type { CollabAgent, CollabRuntimeStatus } from '@/bridge/collab'
import { Button } from '@/components/ui/button'
import { useCollabNavigationStore } from '@/features/collab/collabNavigationStore'
import { useRoomStore } from '@/features/collab/rooms/roomStore'
import { useAgentStore } from './agentStore'
import { agentActualState } from './agentRuntimeState'

export function AgentCard({ agent, runtime, onEdit }: { agent: CollabAgent; runtime: CollabRuntimeStatus | null; onEdit: () => void }) {
  const { t } = useTranslation()
  const setAgenda = useAgentStore((state) => state.setAgenda)
  const setArchived = useAgentStore((state) => state.setArchived)
  const openDirect = useRoomStore((state) => state.openDirect)
  const selectRoom = useCollabNavigationStore((state) => state.selectRoom)
  const [openingChat, setOpeningChat] = useState(false)
  const archived = agent.archivedAt !== null
  const actual = agentActualState(runtime, agent)
  const actualLabel = archived
    ? t('collab.agents.archived')
    : actual.kind === 'running'
      ? t('collab.agents.running')
      : actual.kind === 'error'
        ? `${t('collab.agents.runnerError')}: ${actual.detail ?? t('collab.rooms.unknownFailure')}`
        : t(`collab.agents.${actual.kind}`)
  const tone = archived
    ? 'bg-paper-hover text-ink-faint'
    : actual.kind === 'running'
      ? 'bg-status-success-soft text-status-success-ink'
      : actual.kind === 'error' || actual.kind === 'engineError' || actual.kind === 'engineMissing'
        ? 'bg-status-danger-soft text-status-danger-ink'
        : 'bg-status-warning-soft text-status-warning-ink'

  async function openChat() {
    if (openingChat) return
    setOpeningChat(true)
    const room = await openDirect(agent.id)
    setOpeningChat(false)
    if (room) selectRoom(room.id)
  }

  return (
    <article className="flex min-h-64 flex-col rounded-2xl border border-line bg-paper-hover p-4 transition hover:border-line-strong hover:shadow-sm">
      <div className="flex items-start gap-3">
        <span className="grid size-11 shrink-0 place-items-center rounded-full bg-clay-soft text-clay"><Bot size={20} /></span>
        <div className="min-w-0 flex-1">
          <strong className="block truncate text-base">{agent.displayName}</strong>
          <span className="block truncate text-xs text-ink-faint">{agent.role || `@${agent.id}`}</span>
        </div>
        <Button type="button" size="icon" variant="ghost" className="size-8" aria-label={t('collab.agents.edit')} onClick={onEdit}>
          <Pencil size={14} />
        </Button>
      </div>

      <div className="mt-3 flex flex-wrap gap-1.5 text-[11px]">
        <span className={`inline-flex items-center gap-1.5 rounded-full px-2 py-1 ${tone}`} title={actualLabel}>
          <span className="size-1.5 rounded-full bg-current" />
          <span className="max-w-48 truncate">{actualLabel}</span>
        </span>
        <span className="rounded-full bg-paper px-2 py-1 text-ink-muted">OpenCode</span>
        <span className={`inline-flex items-center gap-1 rounded-full px-2 py-1 ${agent.agendaEnabled ? 'bg-clay-soft text-clay' : 'bg-paper text-ink-faint'}`}>
          <CalendarClock size={11} />{agent.agendaEnabled ? t('collab.agents.proactiveOn') : t('collab.agents.proactiveOff')}
        </span>
      </div>

      <p className="mt-3 line-clamp-3 flex-1 text-sm leading-6 text-ink-muted">{agent.persona}</p>
      <div className="mt-3 rounded-xl bg-paper px-3 py-2">
        <span className="block text-[10px] uppercase tracking-wide text-ink-faint">{t('collab.agents.mainModel')}</span>
        <span className="mt-0.5 block truncate font-mono text-xs text-ink-muted" title={agent.mainModelId}>{agent.mainModelId}</span>
      </div>

      <div className="mt-4 flex items-center gap-2 border-t border-line pt-3">
        <Button type="button" size="sm" variant="outline" className="flex-1" disabled={openingChat} onClick={() => void openChat()}>
          <MessageSquare size={14} />{t('collab.agents.openChat')}
        </Button>
        <Button type="button" size="sm" variant={agent.agendaEnabled ? 'accent' : 'ghost'} disabled={archived} onClick={() => void setAgenda(agent.id, !agent.agendaEnabled)}>
          <CalendarClock size={14} />
        </Button>
        <Button type="button" size="sm" variant="ghost" onClick={() => void setArchived(agent.id, !archived)}>
          {archived ? <RotateCcw size={14} /> : <Archive size={14} />}
        </Button>
      </div>
    </article>
  )
}
