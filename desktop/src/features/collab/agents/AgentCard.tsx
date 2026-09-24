import { Archive, CircleAlert, ClipboardCheck, Clock, Pencil, RotateCcw, Zap } from 'lucide-react'
import { useState } from 'react'
import { useTranslation } from 'react-i18next'

import type { CollabAgent } from '@/bridge/collab'
import { AgentStatusTag, AgentStatusText } from '@/features/collab/components/AgentStatusBadge'
import { agentStatusTag, type StatusTone } from '@/features/collab/components/agentStatus'
import { ParticipantAvatar } from '@/features/collab/components/ParticipantAvatar'
import { cn } from '@/lib/utils'

const DOT_CLASSES: Record<StatusTone, string> = {
  success: 'bg-status-success',
  clay: 'bg-clay',
  danger: 'bg-status-danger',
  neutral: 'bg-line-strong',
}

/** Agent 卡片的操作；由页面接到 store 与导航。 */
export interface AgentCardActions {
  onEdit: () => void
  onArchive: (archived: boolean) => void
  onAgenda: (enabled: boolean) => void
  onMessage: () => Promise<void>
}

/**
 * Agent 卡片（collaboration-desktop.md §8）：头像带状态点、名字与 role、状态与 Engine 标签、
 * 当前状态行（出错时为 danger 提示框）、persona 摘要、模型、Agenda 与私聊；编辑与归档在悬停时出现。
 */
export function AgentCard({ agent, now, actions }: { agent: CollabAgent, now: number, actions: AgentCardActions }) {
  const { t } = useTranslation()
  const [opening, setOpening] = useState(false)
  const archived = agent.archivedAt !== null
  const tone = agentStatusTag(agent.activity).tone

  async function message() {
    if (opening) return
    setOpening(true)
    try {
      await actions.onMessage()
    } finally {
      setOpening(false)
    }
  }

  return (
    <article className="group relative flex flex-col gap-3.5 rounded-2xl border border-line bg-surface p-5 transition hover:shadow-md">
      <div className="absolute right-3.5 top-3.5 hidden gap-1 group-hover:flex group-focus-within:flex">
        {archived ? null : (
          <RoundButton label={t('collab.agents.editNamed', { name: agent.displayName })} onClick={actions.onEdit}><Pencil size={14} /></RoundButton>
        )}
        <RoundButton
          label={t(archived ? 'collab.agents.restoreNamed' : 'collab.agents.archiveNamed', { name: agent.displayName })}
          onClick={() => actions.onArchive(!archived)}
        >
          {archived ? <RotateCcw size={14} /> : <Archive size={14} />}
        </RoundButton>
      </div>
      <div className="flex items-center gap-3.5">
        <span className="relative inline-block shrink-0">
          <ParticipantAvatar name={agent.displayName} isUser={false} size={52} ring={agent.activity.kind === 'working'} />
          <span aria-hidden="true" className={cn('absolute bottom-0 right-0 size-[13px] rounded-full border-[3px] border-surface', DOT_CLASSES[tone])} />
        </span>
        <div className="flex min-w-0 flex-col gap-1">
          <span className="truncate font-serif text-lg font-semibold">
            {agent.displayName} <span className="font-sans text-[13px] font-normal text-ink-faint">@{agent.id}</span>
          </span>
          {agent.role ? <span className="truncate font-serif text-[13px] italic text-ink-soft">{agent.role}</span> : null}
          <span className="flex gap-1.5">
            <AgentStatusTag activity={agent.activity} />
            <span className="rounded-full bg-code-bg px-2 py-0.5 text-[11px] text-ink-soft">OpenCode</span>
          </span>
        </div>
      </div>
      <StateLine agent={agent} now={now} />
      <p className="line-clamp-3 whitespace-pre-line text-[13px] leading-relaxed text-ink-soft">{agent.persona}</p>
      <div className="grid grid-cols-[64px_minmax(0,1fr)] gap-x-2 gap-y-1 text-xs">
        <span className="text-ink-faint">{t('collab.agents.mainModel')}</span><code className="truncate font-mono text-ink-soft" title={agent.mainModelId}>{agent.mainModelId}</code>
        <span className="text-ink-faint">{t('collab.agents.triageModel')}</span><code className="truncate font-mono text-ink-soft" title={agent.triageModelId}>{agent.triageModelId}</code>
      </div>
      <div className="mt-auto flex items-center gap-2 border-t border-line pt-3">
        <label className="flex flex-1 items-center gap-2 text-[13px] text-ink-soft">
          <input type="checkbox" className="size-4 accent-clay" checked={agent.agendaEnabled} disabled={archived} onChange={(event) => actions.onAgenda(event.target.checked)} />
          {t('collab.agents.agenda')}
        </label>
        <button type="button" disabled={archived || opening} className="h-8 rounded-lg bg-ink px-4 text-[13px] font-semibold text-paper disabled:opacity-40" onClick={() => void message()}>
          {t('collab.agents.openChat')}
        </button>
      </div>
    </article>
  )
}

/** 当前状态行：出错时是 danger 提示框，其余是带图标的一行。 */
function StateLine({ agent, now }: { agent: CollabAgent, now: number }) {
  const { activity } = agent
  if (activity.kind === 'archived') return null
  if (activity.kind === 'error') {
    return (
      <div role="alert" className="flex items-start gap-2.5 rounded-xl bg-status-danger-soft px-3 py-2.5 text-[13px] leading-normal text-status-danger-ink">
        <CircleAlert size={15} className="mt-0.5 shrink-0" />
        <span className="min-w-0 break-words"><AgentStatusText activity={activity} now={now} /></span>
      </div>
    )
  }
  const icon = activity.kind === 'working'
    ? <ClipboardCheck size={15} className="text-status-success-ink" />
    : activity.kind === 'queued'
      ? <Zap size={15} className="text-clay" />
      : <Clock size={15} className="text-ink-soft" />
  return (
    <div className="flex items-center gap-2.5 rounded-xl bg-paper-hover px-3 py-2.5 text-[13px]">
      <span className="grid shrink-0">{icon}</span>
      <span className="min-w-0 flex-1 truncate"><AgentStatusText activity={activity} now={now} /></span>
    </div>
  )
}

function RoundButton({ label, onClick, children }: { label: string, onClick: () => void, children: React.ReactNode }) {
  return (
    <button type="button" aria-label={label} title={label} className="grid size-[30px] place-items-center rounded-full border border-line bg-surface text-ink-soft hover:text-ink" onClick={onClick}>
      {children}
    </button>
  )
}
