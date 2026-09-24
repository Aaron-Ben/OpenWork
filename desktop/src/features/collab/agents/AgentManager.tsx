import { Lock, Plus } from 'lucide-react'
import { useState } from 'react'
import { useTranslation } from 'react-i18next'

import type { CollabAgent, CollabAgentInput } from '@/bridge/collab'
import { useCollabNavigationStore } from '@/features/collab/collabNavigationStore'
import { useNow } from '@/features/collab/components/useNow'
import { useRoomStore } from '@/features/collab/rooms/roomStore'
import { useCollabRuntimeStore } from '@/features/collab/runtimeStore'
import { resolveErrorMessage } from '@/lib/commandError'
import { cn } from '@/lib/utils'
import { AgentCard } from './AgentCard'
import { AgentFormDialog } from './AgentFormDialog'
import { agentsInTab, engineTag, statusCounts, type AgentTab } from './agentPageModel'
import { useAgentStore } from './agentStore'

/** 卡片上的已用时间每秒刷新。 */
const TICK_MS = 1_000

const ENGINE_TONES = {
  success: 'bg-status-success-soft text-status-success-ink',
  danger: 'bg-status-danger-soft text-status-danger-ink',
  neutral: 'bg-code-bg text-ink-soft',
  clay: 'bg-clay-soft text-ink',
} as const

/** 正在编辑的 Agent；`agent` 为 `null` 表示新建。 */
interface FormState {
  agent: CollabAgent | null
}

/** Agent 页（collaboration-desktop.md §8）：标题栏计数与 Engine 标签、活跃/已归档标签页、三列卡片与“新建 Agent”格。 */
export function AgentManager() {
  const { t } = useTranslation()
  const agents = useAgentStore((state) => state.agents)
  const create = useAgentStore((state) => state.create)
  const update = useAgentStore((state) => state.update)
  const setAgenda = useAgentStore((state) => state.setAgenda)
  const setArchived = useAgentStore((state) => state.setArchived)
  const storeError = useAgentStore((state) => state.error)
  const runtime = useCollabRuntimeStore((state) => state.status)
  const runtimeError = useCollabRuntimeStore((state) => state.error)
  const openDirect = useRoomStore((state) => state.openDirect)
  const selectRoom = useCollabNavigationStore((state) => state.selectRoom)
  const now = useNow(TICK_MS)
  const [tab, setTab] = useState<AgentTab>('active')
  const [form, setForm] = useState<FormState | null>(null)
  const [error, setError] = useState<string | null>(null)
  const active = agentsInTab(agents, 'active')
  const archived = agentsInTab(agents, 'archived')
  const counts = statusCounts(active)
  const engine = engineTag(runtime)

  async function run(action: () => Promise<void>) {
    setError(null)
    try {
      await action()
    } catch (actionError) {
      setError(resolveErrorMessage(actionError))
    }
  }

  async function submit(input: CollabAgentInput) {
    const agent = form?.agent ?? null
    await run(async () => {
      await (agent ? update(agent.id, input) : create(input))
      setForm(null)
    })
  }

  const countParts = [
    { key: 'working', count: counts.working, className: 'text-status-success-ink' },
    { key: 'queued', count: counts.queued, className: 'text-ink' },
    { key: 'idle', count: counts.idle, className: 'text-ink' },
    { key: 'error', count: counts.error, className: 'text-status-danger-ink' },
  ].filter((part) => part.count > 0 || part.key === 'working')

  return (
    <section className="relative flex min-w-0 flex-1 flex-col bg-paper">
      <header data-tauri-drag-region="deep" className="flex shrink-0 items-end gap-4 px-10 pb-4 pt-7">
        <div className="flex flex-1 flex-col gap-1.5">
          <h1 className="font-serif text-2xl font-semibold">{t('collab.agents.title')}</h1>
          <span className="flex flex-wrap gap-3.5 text-[13px] text-ink-soft">
            {countParts.map((part) => (
              <span key={part.key}><strong className={part.className}>{part.count}</strong> {t(`collab.agents.counts.${part.key}`)}</span>
            ))}
          </span>
        </div>
        <span className={cn('flex h-[34px] items-center gap-2 rounded-lg px-3 text-xs font-semibold', ENGINE_TONES[engine.tone])} title={engine.values.reason}>
          <Lock size={14} />
          <span className="max-w-72 truncate">{t(engine.key, engine.values)}</span>
        </span>
        <button type="button" className="flex h-[34px] items-center gap-1.5 rounded-lg bg-ink px-3.5 text-[13px] font-semibold text-paper" onClick={() => setForm({ agent: null })}>
          <Plus size={15} />{t('collab.agents.create')}
        </button>
      </header>
      <div role="tablist" aria-label={t('collab.agents.tabs')} className="flex shrink-0 gap-1 border-b border-line px-10">
        {(['active', 'archived'] as const).map((item) => (
          <button
            key={item}
            type="button"
            role="tab"
            aria-selected={tab === item}
            className={cn('h-9 border-b-2 px-3 text-[13px]', tab === item ? 'border-clay font-semibold text-ink' : 'border-transparent text-ink-soft')}
            onClick={() => setTab(item)}
          >
            {t(`collab.agents.tab.${item}`, { count: item === 'active' ? active.length : archived.length })}
          </button>
        ))}
      </div>
      <div className="flex-1 overflow-y-auto px-10 py-6">
        {[error, storeError, runtimeError].filter(Boolean).map((message) => (
          <p key={message} className="mb-3 text-sm text-status-danger-ink">{message}</p>
        ))}
        <div className="grid grid-cols-1 content-start gap-5 md:grid-cols-2 xl:grid-cols-3">
          {(tab === 'active' ? active : archived).map((agent) => (
            <AgentCard
              key={agent.id}
              agent={agent}
              now={now}
              actions={{
                onEdit: () => setForm({ agent }),
                onArchive: (next) => void run(() => setArchived(agent.id, next)),
                onAgenda: (enabled) => void run(() => setAgenda(agent.id, enabled)),
                onMessage: () => run(async () => {
                  const room = await openDirect(agent.id)
                  if (room) selectRoom(room.id)
                }),
              }}
            />
          ))}
          {tab === 'active' ? <HireCard onClick={() => setForm({ agent: null })} /> : null}
        </div>
        {tab === 'archived' && archived.length === 0 ? <p className="py-20 text-center text-sm text-ink-faint">{t('collab.agents.noArchived')}</p> : null}
      </div>
      {form ? (
        <AgentFormDialog agent={form.agent} error={error} onSubmit={submit} onClose={() => { setForm(null); setError(null) }} />
      ) : null}
    </section>
  )
}

/** 网格最后一格“新建 Agent”（Cumora `AgentsView.tsx` 的 `HireCard`）。 */
export function HireCard({ onClick }: { onClick: () => void }) {
  const { t } = useTranslation()
  return (
    <button type="button" className="flex min-h-64 flex-col items-center justify-center gap-2.5 rounded-2xl border-[1.5px] border-dashed border-line-strong p-5 text-ink-soft hover:bg-paper-hover" onClick={onClick}>
      <span className="grid size-[52px] place-items-center rounded-full bg-paper-hover text-clay"><Plus size={22} /></span>
      <span className="font-serif text-lg font-semibold text-ink">{t('collab.agents.create')}</span>
      <span className="max-w-60 text-center text-[13px] leading-normal">{t('collab.agents.hireHint')}</span>
    </button>
  )
}
