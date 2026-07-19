import { Activity, Bot, Wrench } from 'lucide-react'
import { useTranslation } from 'react-i18next'

import { formatBeijingDateTime } from '../../../lib/dateTime'
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
      <div data-trace-loading="true" className="grid gap-2" aria-label={t('activity.loading')}>
        {[0, 1, 2].map((index) => (
          <div key={index} className="h-14 animate-pulse rounded-xl border border-line bg-surface" />
        ))}
      </div>
    )
  }
  if (items.length === 0) {
    return (
      <div className="rounded-xl border border-dashed border-line py-12 text-center">
        <Activity size={22} className="mx-auto text-ink-faint" />
        <p className="mt-2.5 text-sm text-ink-faint">{t('activity.empty')}</p>
      </div>
    )
  }

  return (
    <div className="overflow-hidden rounded-xl border border-line bg-paper">
      {items.map((item, index) => (
        <button
          key={item.turnId}
          type="button"
          data-trace-row={item.turnId}
          className={`flex w-full items-center gap-3 px-4 py-3 text-left transition-colors hover:bg-paper-hover focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-clay/35 ${
            index > 0 ? 'border-t border-line' : ''
          }`}
          onClick={() => onOpen(item)}
        >
          <span className={`size-2 shrink-0 rounded-full ${statusDot(item.status)}`} />
          <span className="min-w-0 flex-1">
            <span className="flex items-center gap-2">
              <span className="truncate text-sm font-medium text-ink">{item.title}</span>
              <StatusBadge status={item.status} />
            </span>
            <span className="mt-0.5 block truncate font-mono text-[11px] text-ink-faint">
              {item.workingDirectory || item.sessionId}
            </span>
          </span>
          <span className="hidden shrink-0 items-center gap-3 text-[11px] text-ink-faint lg:flex">
            <span className="inline-flex items-center gap-1"><Bot size={12} />{t('activity.modelCalls', { count: item.modelCallCount })}</span>
            <span className="inline-flex items-center gap-1"><Wrench size={12} />{t('activity.toolCalls', { count: item.toolCallCount })}</span>
            <span className="max-w-36 truncate">{item.resolvedModelName}</span>
          </span>
          <span className="shrink-0 text-right">
            <span className="block text-xs tabular-nums text-ink-soft">{formatDuration(item.durationMs)}</span>
            <span className="mt-0.5 block text-[11px] text-ink-faint">{formatBeijingDateTime(item.startedAt)}</span>
          </span>
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

export function formatDuration(durationMs: number | null): string {
  if (durationMs == null) return '—'
  if (durationMs < 1000) return `${Math.round(durationMs)} ms`
  return `${(durationMs / 1000).toFixed(2)} s`
}
