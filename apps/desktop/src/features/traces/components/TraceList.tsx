import { Activity, Bot, Clock3, Wrench } from 'lucide-react'
import { useTranslation } from 'react-i18next'

import type { TraceListItem } from '../traceViewModel'

interface TraceListProps {
  items: TraceListItem[]
  loading: boolean
  onOpen: (item: TraceListItem) => void
}

export function TraceList({ items, loading, onOpen }: TraceListProps) {
  const { t } = useTranslation()
  if (loading) {
    return (
      <div data-trace-loading="true" className="grid gap-3" aria-label={t('activity.loading')}>
        {[0, 1, 2].map((index) => (
          <div key={index} className="h-24 animate-pulse rounded-2xl border border-line bg-surface" />
        ))}
      </div>
    )
  }
  if (items.length === 0) {
    return (
      <div className="rounded-2xl border border-dashed border-line py-14 text-center">
        <Activity size={24} className="mx-auto text-ink-faint" />
        <p className="mt-3 text-sm text-ink-faint">{t('activity.empty')}</p>
      </div>
    )
  }

  return (
    <div className="grid gap-3">
      {items.map((item) => (
        <button
          key={item.turnId}
          type="button"
          data-trace-row={item.turnId}
          className="group w-full rounded-2xl border border-line bg-surface px-5 py-4 text-left transition hover:-translate-y-0.5 hover:border-clay/50 hover:shadow-sm focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-clay/35"
          onClick={() => onOpen(item)}
        >
          <div className="flex items-start gap-4">
            <span className={`mt-1.5 size-2.5 shrink-0 rounded-full ${statusDot(item.status)}`} />
            <span className="min-w-0 flex-1">
              <span className="flex flex-wrap items-center gap-2">
                <span className="truncate text-sm font-semibold text-ink">{item.title}</span>
                <StatusBadge status={item.status} />
              </span>
              <span className="mt-1 block truncate font-mono text-[11px] text-ink-faint">
                {item.workingDirectory || item.sessionId}
              </span>
              <span className="mt-3 flex flex-wrap items-center gap-x-4 gap-y-1 text-xs text-ink-faint">
                <span className="inline-flex items-center gap-1.5"><Bot size={13} />{t('activity.modelCalls', { count: item.modelCallCount })}</span>
                <span className="inline-flex items-center gap-1.5"><Wrench size={13} />{t('activity.toolCalls', { count: item.toolCallCount })}</span>
                <span>{item.resolvedModelName}</span>
              </span>
            </span>
            <span className="hidden shrink-0 text-right text-xs text-ink-faint sm:block">
              <span className="block">{formatDate(item.startedAt)}</span>
              <span className="mt-2 inline-flex items-center gap-1"><Clock3 size={12} />{formatDuration(item.durationMs)}</span>
            </span>
          </div>
        </button>
      ))}
    </div>
  )
}

function StatusBadge({ status }: { status: string }) {
  const { t } = useTranslation()
  return (
    <span className={`rounded-full px-2 py-0.5 text-[10px] font-semibold ${statusBadge(status)}`}>
      {t(`activity.status.${status}` as 'activity.status.completed', { defaultValue: status })}
    </span>
  )
}

function statusDot(status: string): string {
  if (status === 'completed') return 'bg-status-success'
  if (status === 'running') return 'bg-status-warning'
  if (status === 'cancelled' || status === 'interrupted') return 'bg-ink-faint'
  return 'bg-status-danger'
}

function statusBadge(status: string): string {
  if (status === 'completed') return 'bg-status-success-soft text-status-success'
  if (status === 'running') return 'bg-status-warning-soft text-status-warning-ink'
  if (status === 'cancelled' || status === 'interrupted') return 'bg-paper-hover text-ink-soft'
  return 'bg-status-danger-soft text-status-danger-ink'
}

function formatDate(value: string): string {
  return new Intl.DateTimeFormat(undefined, {
    month: 'short', day: 'numeric', hour: '2-digit', minute: '2-digit',
  }).format(Date.parse(value))
}

export function formatDuration(durationMs: number | null): string {
  if (durationMs == null) return '—'
  if (durationMs < 1000) return `${Math.round(durationMs)} ms`
  return `${(durationMs / 1000).toFixed(2)} s`
}
