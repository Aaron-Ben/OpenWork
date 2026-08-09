import { useMemo } from 'react'
import { useTranslation } from 'react-i18next'

import { buildTraceDashboardMetrics, type TraceListItem } from '../traceViewModel'

export function TraceDashboardSummary({
  items,
  now = Date.now(),
}: {
  items: TraceListItem[]
  now?: number
}) {
  const { t } = useTranslation()
  const metrics = useMemo(() => buildTraceDashboardMetrics(items, now), [items, now])
  const cards = [
    { id: 'runCount', label: t('activity.dashboard.todayRuns'), value: String(metrics.runCount) },
    {
      id: 'successRate',
      label: t('activity.dashboard.successRate'),
      value: metrics.successRate == null ? '—' : `${metrics.successRate}%`,
      note: metrics.failedCount > 0
        ? t('activity.dashboard.failedRuns', { count: metrics.failedCount })
        : undefined,
    },
    {
      id: 'medianDuration',
      label: t('activity.dashboard.medianDuration'),
      value: metrics.medianDurationMs == null ? '—' : formatDashboardDuration(metrics.medianDurationMs),
    },
    { id: 'totalTokens', label: t('activity.dashboard.totalTokens'), value: formatCompactTokens(metrics.totalTokens) },
  ]

  return (
    <section
      data-trace-dashboard-summary="true"
      aria-label={t('activity.dashboard.summary')}
    >
      <div className="grid grid-cols-4 gap-3 max-[780px]:grid-cols-2 max-[440px]:grid-cols-1">
        {cards.map((card) => (
          <div key={card.id} className="min-w-0 rounded-2xl bg-surface px-5 py-4">
            <div className="text-[11px] font-semibold text-ink-faint">{card.label}</div>
            <div className="mt-1 flex min-w-0 items-baseline gap-2">
              <strong data-dashboard-value={card.id} className="truncate font-serif text-2xl font-semibold tabular-nums text-ink">{card.value}</strong>
              {card.note ? <span className="truncate text-[11px] font-semibold text-clay">{card.note}</span> : null}
            </div>
          </div>
        ))}
      </div>
    </section>
  )
}

function formatDashboardDuration(durationMs: number): string {
  if (durationMs < 1_000) return `${Math.round(durationMs)} ms`
  return `${Math.round(durationMs / 1_000)} s`
}

function formatCompactTokens(tokens: number): string {
  if (tokens >= 1_000_000) return `${(tokens / 1_000_000).toFixed(1)} M`
  if (tokens >= 1_000) return `${(tokens / 1_000).toFixed(1)} K`
  return tokens.toLocaleString()
}
