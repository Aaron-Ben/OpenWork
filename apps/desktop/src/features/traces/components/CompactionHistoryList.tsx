import { useTranslation } from 'react-i18next'

import { formatBeijingDateTime } from '../../../lib/dateTime'
import type { CompactionHistoryItem } from '../traceViewModel'
import { formatDuration } from './TraceList'

interface CompactionHistoryListProps {
  items: CompactionHistoryItem[]
  loading: boolean
  error: string | null
}

/**
 * Compaction history for one Session, newest first.
 *
 * A manual compaction has no Turn, so it never appears in the Turn-scoped Trace
 * views; this is the only place its Span is visible.
 */
export function CompactionHistoryList({ items, loading, error }: CompactionHistoryListProps) {
  const { t } = useTranslation()

  if (error) {
    return (
      <p className="rounded-xl bg-status-danger-soft p-3 text-sm text-status-danger-ink" role="alert">
        {error}
      </p>
    )
  }
  if (loading && items.length === 0) {
    return (
      <div
        data-compaction-loading="true"
        className="h-14 animate-pulse rounded-xl border border-line bg-surface"
        aria-label={t('chat.compactionHistory.loading')}
      />
    )
  }
  if (items.length === 0) {
    return (
      <p className="rounded-xl border border-dashed border-line px-3 py-6 text-center text-xs text-ink-faint">
        {t('chat.compactionHistory.empty')}
      </p>
    )
  }

  return (
    <ul className="grid gap-2">
      {items.map((item) => (
        <li
          key={item.span.id}
          data-compaction-row={item.span.id}
          className="rounded-xl border border-line bg-surface/40 px-3 py-2.5"
        >
          <div className="flex items-center gap-2">
            <span className={`size-2 shrink-0 rounded-full ${statusDot(item.span.status)}`} />
            <span className="text-sm font-medium text-ink">
              {t(`chat.compactionHistory.trigger.${item.trigger}` as 'chat.compactionHistory.trigger.manual', {
                defaultValue: item.trigger,
              })}
            </span>
            {item.turnId ? null : (
              <span className="rounded-full bg-paper-hover px-2 py-0.5 text-[10px] text-ink-soft">
                {t('chat.compactionHistory.sessionScoped')}
              </span>
            )}
            <span className="ml-auto shrink-0 text-[11px] tabular-nums text-ink-faint">
              {formatDuration(item.durationMs)}
            </span>
          </div>
          <div className="mt-1 flex flex-wrap items-center gap-x-3 gap-y-1 text-[11px] text-ink-faint">
            <span>{formatBeijingDateTime(item.span.startedAt)}</span>
            <span className="font-mono tabular-nums">{formatReclaim(item)}</span>
            {item.span.errorCode ? (
              <span className="font-mono text-status-danger">{item.span.errorCode}</span>
            ) : null}
          </div>
        </li>
      ))}
    </ul>
  )
}

/** `12.0k → 900 (−11.1k)`, or an em dash when the Span predates the measurement. */
export function formatReclaim(item: CompactionHistoryItem): string {
  const { conversationTokensBefore: before, conversationTokensAfter: after } = item
  if (before == null || after == null) return '—'
  const reclaimed = item.reclaimedTokens ?? Math.max(0, before - after)
  return `${compactTokens(before)} → ${compactTokens(after)} (−${compactTokens(reclaimed)})`
}

function compactTokens(value: number): string {
  return value >= 1000 ? `${(value / 1000).toFixed(1)}k` : String(value)
}

function statusDot(status: string): string {
  if (status === 'succeeded') return 'bg-status-success'
  if (status === 'running') return 'bg-status-warning'
  if (status === 'cancelled') return 'bg-ink-faint'
  return 'bg-status-danger'
}
