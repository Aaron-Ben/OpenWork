import { AlertCircle, CheckCircle2, Clock3, LoaderCircle, RefreshCw } from 'lucide-react'
import { useEffect } from 'react'
import { useTranslation } from 'react-i18next'

import type { CollabRun, CollabRunEvent } from '@/bridge/collab'
import { Button } from '@/components/ui/button'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { useAgentStore } from '@/features/collab/agents/agentStore'
import { useObservabilityStore } from './observabilityStore'

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

  useEffect(() => {
    void refresh()
    const timer = globalThis.setInterval(() => void refresh(), 3_000)
    return () => globalThis.clearInterval(timer)
  }, [agentFilter, refresh, statusFilter])

  return (
    <section className="flex min-w-0 flex-1 flex-col overflow-hidden bg-paper">
      <header data-tauri-drag-region="deep" className="flex h-12 shrink-0 items-center justify-between border-b border-line px-6">
        <div className="flex items-baseline gap-3">
          <h1 className="font-serif text-lg font-semibold">{t('collab.observability.title')}</h1>
          <span className="text-xs text-ink-faint">{t('collab.observability.subtitle')}</span>
        </div>
        <Button type="button" size="sm" variant="ghost" onClick={() => void refresh()}>
          <RefreshCw size={14} />{t('collab.observability.refresh')}
        </Button>
      </header>

      <div className="grid min-h-0 flex-1 grid-cols-[minmax(280px,360px)_minmax(0,1fr)]">
        <aside className="flex min-h-0 flex-col border-r border-line">
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
      </div>
    </section>
  )
}

function RunRow({
  run,
  selected,
  agentName,
  onClick,
}: {
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
        <StatusBadge status={run.status} />
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

function TraceDetail({ run, events }: { run: CollabRun; events: CollabRunEvent[] }) {
  const { t } = useTranslation()
  const totalTokens = (run.inputTokens ?? 0)
    + (run.cachedInputTokens ?? 0)
    + (run.cacheCreationInputTokens ?? 0)
    + (run.outputTokens ?? 0)
  return (
    <article className="mx-auto grid max-w-5xl gap-5 p-6">
      <div className="grid gap-2">
        <div className="flex flex-wrap items-center gap-2">
          <h2 className="font-serif text-xl font-semibold">@{run.agentId}</h2>
          <StatusBadge status={run.status} />
          {run.outcome ? <span className="rounded-full bg-paper-hover px-2 py-0.5 text-xs text-ink-muted">{run.outcome}</span> : null}
        </div>
        <p className="break-all font-mono text-xs text-ink-faint">{run.id}</p>
        {run.triggerReason ? <p className="text-sm text-ink-muted">{run.triggerReason}</p> : null}
      </div>

      <dl className="grid grid-cols-2 gap-2 sm:grid-cols-3 xl:grid-cols-6">
        <Metric label={t('collab.observability.duration')} value={formatDuration(run.durationMs, t)} />
        <Metric label={t('collab.observability.tokens')} value={formatCount(totalTokens)} />
        <Metric label={t('collab.observability.tools')} value={formatCount(run.toolCalls)} />
        <Metric label={t('collab.observability.inbox')} value={formatCount(run.inboxMessageCount)} />
        <Metric label={t('collab.observability.events')} value={formatCount(run.eventCount)} />
        <Metric label={t('collab.observability.trigger')} value={run.trigger} />
      </dl>

      <dl className="grid gap-x-6 gap-y-2 rounded-2xl border border-line p-4 text-sm sm:grid-cols-2">
        <Property label={t('collab.observability.engine')} value={run.engineId} />
        <Property label={t('collab.observability.configuredModel')} value={run.mainModelId} />
        <Property label={t('collab.observability.observedModel')} value={run.observedModelId ?? '—'} />
        <Property label={t('collab.observability.startedAt')} value={formatDateTime(run.startedAt)} />
      </dl>

      {run.errorMessage ? (
        <div className="rounded-2xl border border-red-200 bg-red-50 p-4 text-sm text-red-700">
          <strong>{run.errorCode ?? t('collab.observability.unknownError')}</strong>
          <p className="mt-1 whitespace-pre-wrap">{run.errorMessage}</p>
        </div>
      ) : null}

      <section className="grid gap-3">
        <h3 className="font-serif text-lg font-semibold">{t('collab.observability.timeline')}</h3>
        {events.length === 0 ? <p className="text-sm text-ink-faint">{t('collab.observability.noEvents')}</p> : null}
        <div className="relative grid gap-3 before:absolute before:bottom-4 before:left-[7px] before:top-4 before:w-px before:bg-line">
          {events.map((event) => <EventRow key={event.id} event={event} />)}
        </div>
      </section>
    </article>
  )
}

function EventRow({ event }: { event: CollabRunEvent }) {
  const { t } = useTranslation()
  const details = eventSummary(event, t)
  return (
    <article className="relative grid grid-cols-[15px_minmax(0,1fr)] gap-3">
      <span className={`z-10 mt-1.5 size-[15px] rounded-full border-2 border-paper ${event.level === 'error' ? 'bg-red-500' : event.level === 'warning' ? 'bg-amber-500' : 'bg-clay'}`} />
      <div className="rounded-2xl border border-line bg-paper p-3">
        <div className="flex flex-wrap items-start justify-between gap-2">
          <div>
            <strong className="text-sm">{eventLabel(event.kind, event.data, t)}</strong>
            <span className="ml-2 font-mono text-[11px] text-ink-faint">{event.source}</span>
          </div>
          <time className="text-xs text-ink-faint">{formatTime(event.createdAt)}</time>
        </div>
        {details ? <p className="mt-1 text-sm text-ink-muted">{details}</p> : null}
        {Object.keys(event.data).length > 0 ? (
          <details className="mt-2 text-xs">
            <summary className="cursor-pointer select-none text-ink-faint">{t('collab.observability.rawData')}</summary>
            <pre className="mt-2 max-h-72 overflow-auto rounded-xl bg-paper-hover p-3 font-mono text-[11px] leading-relaxed text-ink-muted">{JSON.stringify(event.data, null, 2)}</pre>
          </details>
        ) : null}
      </div>
    </article>
  )
}

function StatusBadge({ status }: { status: string }) {
  const { t } = useTranslation()
  const successful = status === 'completed'
  const running = status === 'running'
  return (
    <span className={`inline-flex items-center gap-1 rounded-full px-2 py-0.5 text-[11px] ${running ? 'bg-blue-50 text-blue-700' : successful ? 'bg-emerald-50 text-emerald-700' : 'bg-red-50 text-red-700'}`}>
      {running ? <Clock3 size={11} /> : successful ? <CheckCircle2 size={11} /> : <AlertCircle size={11} />}
      {t(`collab.observability.status.${status}`, { defaultValue: status })}
    </span>
  )
}

function Metric({ label, value }: { label: string; value: string }) {
  return <div className="rounded-xl bg-paper-hover p-3"><dt className="text-xs text-ink-faint">{label}</dt><dd className="mt-1 truncate text-sm font-semibold">{value}</dd></div>
}

function Property({ label, value }: { label: string; value: string }) {
  return <div className="grid grid-cols-[120px_minmax(0,1fr)] gap-2"><dt className="text-ink-faint">{label}</dt><dd className="truncate font-mono text-xs">{value}</dd></div>
}

function FilterSelect({
  label,
  value,
  onChange,
  options,
}: {
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

function eventLabel(kind: string, data: Record<string, unknown>, t: ReturnType<typeof useTranslation>['t']): string {
  if (kind === 'triage.completed') {
    return data.actionable
      ? t('collab.observability.event.triageActionable')
      : t('collab.observability.event.triageIgnored')
  }
  if (kind === 'command.completed') {
    const command = nestedString(data, 'response', 'effects', '0', 'type')
      ?? nestedString(data, 'response', 'result', 'type')
      ?? 'unknown'
    return t('collab.observability.event.commandCompleted', { command })
  }
  return t(`collab.observability.event.${kind.replace('.', '_')}`, { defaultValue: kind })
}

function eventSummary(event: CollabRunEvent, t: ReturnType<typeof useTranslation>['t']): string | null {
  const duration = numberValue(event.data.durationMs)
  const model = stringValue(event.data.observedModelId) ?? stringValue(event.data.configuredModelId)
  const reason = stringValue(event.data.reason) ?? stringValue(event.data.errorMessage)
  const parts = [
    model,
    duration === null ? null : formatDuration(duration, t),
    reason,
  ].filter((part): part is string => Boolean(part))
  return parts.length > 0 ? parts.join(' · ') : null
}

function nestedString(value: Record<string, unknown>, ...path: string[]): string | null {
  let current: unknown = value
  for (const key of path) {
    if (typeof current !== 'object' || current === null) return null
    current = (current as Record<string, unknown>)[key]
  }
  return stringValue(current)
}

function stringValue(value: unknown): string | null {
  return typeof value === 'string' && value.length > 0 ? value : null
}

function numberValue(value: unknown): number | null {
  return typeof value === 'number' && Number.isFinite(value) ? value : null
}

function formatDuration(milliseconds: number, t: ReturnType<typeof useTranslation>['t']): string {
  if (milliseconds < 1_000) return t('collab.observability.milliseconds', { count: milliseconds })
  if (milliseconds < 60_000) return `${(milliseconds / 1_000).toFixed(1)}s`
  return `${Math.floor(milliseconds / 60_000)}m ${Math.floor((milliseconds % 60_000) / 1_000)}s`
}

function formatCount(value: number): string {
  return new Intl.NumberFormat().format(value)
}

function formatTime(value: string): string {
  return new Intl.DateTimeFormat(undefined, { hour: '2-digit', minute: '2-digit', second: '2-digit' }).format(new Date(value))
}

function formatDateTime(value: string): string {
  return new Intl.DateTimeFormat(undefined, { dateStyle: 'medium', timeStyle: 'medium' }).format(new Date(value))
}
