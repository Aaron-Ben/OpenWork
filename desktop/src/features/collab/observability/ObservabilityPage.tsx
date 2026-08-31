import { AlertCircle, LoaderCircle, RefreshCw } from 'lucide-react'
import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'

import type { CollabRun } from '@/bridge/collab'
import { Button } from '@/components/ui/button'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { useAgentStore } from '@/features/collab/agents/agentStore'
import { ResizableSidebarLayout } from '@/features/collab/components/ResizableSidebarLayout'
import { useObservabilityStore } from './observabilityStore'
import { RunStatusBadge, TraceDetail } from './TraceDetail'

const RUN_STATUSES = ['running', 'completed', 'failed', 'cancelled', 'interrupted'] as const
const ALL_FILTER = '__all__'

export function ObservabilityPage() {
  const { t } = useTranslation()
  const agents = useAgentStore((state) => state.agents)
  const runs = useObservabilityStore((state) => state.runs)
  const trace = useObservabilityStore((state) => state.trace)
  const selectedRunId = useObservabilityStore((state) => state.selectedRunId)
  const agentFilter = useObservabilityStore((state) => state.agentFilter)
  const statusFilter = useObservabilityStore((state) => state.statusFilter)
  const loading = useObservabilityStore((state) => state.loading)
  const error = useObservabilityStore((state) => state.error)
  const setAgentFilter = useObservabilityStore((state) => state.setAgentFilter)
  const setStatusFilter = useObservabilityStore((state) => state.setStatusFilter)
  const selectRun = useObservabilityStore((state) => state.selectRun)
  const refresh = useObservabilityStore((state) => state.refresh)
  const [autoRefresh, setAutoRefresh] = useState(true)

  useEffect(() => {
    void refresh()
  }, [agentFilter, refresh, statusFilter])

  useEffect(() => {
    if (!autoRefresh) return
    const refreshWhenVisible = () => {
      if (document.visibilityState === 'visible') void refresh()
    }
    const timer = globalThis.setInterval(refreshWhenVisible, 3_000)
    document.addEventListener('visibilitychange', refreshWhenVisible)
    return () => {
      globalThis.clearInterval(timer)
      document.removeEventListener('visibilitychange', refreshWhenVisible)
    }
  }, [autoRefresh, refresh])

  return (
    <section className="flex min-w-0 flex-1 flex-col overflow-hidden bg-paper">
      <header data-tauri-drag-region="deep" className="flex h-12 shrink-0 items-center justify-between border-b border-line px-6">
        <div className="flex items-baseline gap-3">
          <h1 className="font-serif text-lg font-semibold">{t('collab.observability.title')}</h1>
          <span className="text-xs text-ink-faint">{t('collab.observability.subtitle')}</span>
        </div>
        <div className="flex items-center gap-3">
          <label className="flex cursor-pointer items-center gap-2 text-xs text-ink-faint">
            <input className="size-4 accent-clay" type="checkbox" checked={autoRefresh} onChange={(event) => setAutoRefresh(event.target.checked)} />
            {t('collab.observability.autoRefresh')}
          </label>
          <Button type="button" size="sm" variant="ghost" onClick={() => void refresh()}>
            <RefreshCw size={14} />{t('collab.observability.refresh')}
          </Button>
        </div>
      </header>

      <ResizableSidebarLayout
        className="min-h-0 flex-1"
        storageKey="observability"
        defaultWidth={360}
        minWidth={280}
        maxWidth={560}
        resizeLabel={t('collab.observability.resizeSidebar')}
        sidebar={(
          <aside className="flex h-full min-h-0 flex-col border-r border-line">
            <div className="grid grid-cols-2 gap-2 border-b border-line p-3">
              <FilterSelect
                label={t('collab.observability.agentFilter')}
                value={agentFilter ?? ALL_FILTER}
                onChange={(value) => setAgentFilter(value === ALL_FILTER ? null : value)}
                options={[
                  { value: ALL_FILTER, label: t('collab.observability.allAgents') },
                  ...agents.map((agent) => ({ value: agent.id, label: agent.displayName })),
                ]}
              />
              <FilterSelect
                label={t('collab.observability.statusFilter')}
                value={statusFilter ?? ALL_FILTER}
                onChange={(value) => setStatusFilter(value === ALL_FILTER ? null : value)}
                options={[
                  { value: ALL_FILTER, label: t('collab.observability.allStatuses') },
                  ...RUN_STATUSES.map((status) => ({
                    value: status,
                    label: t(`collab.observability.status.${status}`),
                  })),
                ]}
              />
            </div>
            <div className="min-h-0 flex-1 overflow-y-auto p-2">
              {loading ? (
                <div className="flex items-center justify-center gap-2 py-12 text-sm text-ink-faint">
                  <LoaderCircle className="animate-spin" size={16} />{t('collab.observability.loading')}
                </div>
              ) : runs.length === 0 ? (
                <p className="py-12 text-center text-sm text-ink-faint">{t('collab.observability.empty')}</p>
              ) : runs.map((run) => (
                <RunRow
                  key={run.id}
                  run={run}
                  selected={run.id === selectedRunId}
                  agentName={agents.find((agent) => agent.id === run.agentId)?.displayName}
                  onClick={() => void selectRun(run.id)}
                />
              ))}
            </div>
          </aside>
        )}
      >
        <main className="min-w-0 overflow-y-auto">
          {error ? (
            <div className="m-5 flex items-start gap-2 rounded-xl border border-red-200 bg-red-50 p-3 text-sm text-red-700">
              <AlertCircle className="mt-0.5 shrink-0" size={16} />{error}
            </div>
          ) : null}
          {trace ? <TraceDetail run={trace.run} events={trace.events} /> : (
            <div className="grid h-full place-items-center text-sm text-ink-faint">
              {t('collab.observability.selectRun')}
            </div>
          )}
        </main>
      </ResizableSidebarLayout>
    </section>
  )
}

function RunRow({ run, selected, agentName, onClick }: {
  run: CollabRun
  selected: boolean
  agentName?: string
  onClick: () => void
}) {
  const { t } = useTranslation()
  return (
    <button
      type="button"
      className={`mb-1 grid w-full gap-1 rounded-xl px-3 py-2.5 text-left ${selected ? 'bg-paper-hover shadow-sm ring-1 ring-line' : 'hover:bg-paper-hover'}`}
      onClick={onClick}
    >
      <span className="flex items-center justify-between gap-3">
        <strong className="truncate text-sm">{agentName ?? `@${run.agentId}`}</strong>
        <RunStatusBadge status={run.status} />
      </span>
      <span className="truncate font-mono text-[11px] text-ink-faint">{run.stage}</span>
      <span className="flex justify-between text-xs text-ink-muted">
        <span>{formatTime(run.startedAt)}</span>
        <span>{formatDuration(run.durationMs, t)}</span>
      </span>
      {run.errorMessage ? <span className="line-clamp-2 text-xs text-red-600">{run.errorMessage}</span> : null}
    </button>
  )
}

function FilterSelect({ label, value, onChange, options }: {
  label: string
  value: string
  onChange: (value: string) => void
  options: Array<{ value: string; label: string }>
}) {
  return (
    <div className="grid min-w-0 gap-1.5 text-[11px] text-ink-faint">
      <span>{label}</span>
      <Select value={value} onValueChange={onChange}>
        <SelectTrigger
          aria-label={label}
          className="h-9 w-full rounded-xl border border-line bg-paper px-3 text-xs font-medium shadow-sm hover:border-ink-faint hover:bg-paper-hover focus-visible:ring-2 focus-visible:ring-clay/20 data-[state=open]:border-clay data-[state=open]:bg-paper-hover"
        >
          <SelectValue />
        </SelectTrigger>
        <SelectContent sideOffset={6} align="start">
          {options.map((option) => (
            <SelectItem key={option.value} value={option.value} className="text-xs">
              {option.label}
            </SelectItem>
          ))}
        </SelectContent>
      </Select>
    </div>
  )
}

function formatDuration(milliseconds: number, t: ReturnType<typeof useTranslation>['t']): string {
  if (milliseconds < 1_000) return t('collab.observability.milliseconds', { count: milliseconds })
  if (milliseconds < 60_000) return `${(milliseconds / 1_000).toFixed(1)}s`
  return `${Math.floor(milliseconds / 60_000)}m ${Math.floor((milliseconds % 60_000) / 1_000)}s`
}

function formatTime(value: string): string {
  return new Intl.DateTimeFormat(undefined, { hour: '2-digit', minute: '2-digit', second: '2-digit' }).format(new Date(value))
}
