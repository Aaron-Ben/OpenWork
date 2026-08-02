import { Activity, ArrowDown, ArrowUp, ArrowUpDown, Minimize2 } from 'lucide-react'
import { useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'

import { formatBeijingDateTime } from '@/lib/dateTime'
import {
  DEFAULT_TRACE_SORT,
  sortTraceListItems,
  type TraceListItem,
  type TraceSort,
  type TraceSortKey,
} from '../traceViewModel'

// 表头与数据行共享同一栅格模板；容器允许横向滚动，窄屏不压列。
const TABLE_GRID =
  'grid-cols-[100px_minmax(0,1.4fr)_minmax(96px,0.7fr)_68px_68px_76px_76px_168px]'

interface TraceListProps {
  items: TraceListItem[]
  loading: boolean
  onOpen: (item: TraceListItem) => void
}

export function TraceList({ items, loading, onOpen }: TraceListProps) {
  const { t } = useTranslation()
  const [sort, setSort] = useState<TraceSort>(DEFAULT_TRACE_SORT)
  const sorted = useMemo(() => sortTraceListItems(items, sort), [items, sort])

  const handleSort = (key: TraceSortKey) => {
    setSort((current) => current.key === key
      ? { key, direction: current.direction === 'asc' ? 'desc' : 'asc' }
      // 文本列先升序，数值/时间列先降序（最新、最慢、最贵优先）。
      : { key, direction: key === 'resolvedModelName' ? 'asc' : 'desc' })
  }

  if (loading) {
    return (
      <div data-trace-loading="true" className="grid gap-2" aria-label={t('activity.loading')}>
        {[0, 1, 2].map((index) => (
          <div key={index} className="h-11 animate-pulse rounded-xl border border-line bg-surface" />
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
    <div className="overflow-x-auto rounded-xl border border-line bg-paper">
      <div role="table" aria-label={t('activity.title')} className="min-w-[900px]">
        <div role="rowgroup">
          <div role="row" className={`grid ${TABLE_GRID} items-center gap-3 border-b border-line px-4 py-2`}>
            <span role="columnheader" className="text-[11px] font-medium text-ink-faint">
              {t('activity.table.status')}
            </span>
            <span role="columnheader" className="text-[11px] font-medium text-ink-faint">
              {t('activity.table.run')}
            </span>
            <SortHeader column="resolvedModelName" sort={sort} onSort={handleSort} label={t('activity.table.model')} />
            <SortHeader column="modelSubmissionCount" align="right" sort={sort} onSort={handleSort} label={t('activity.table.modelCalls')} />
            <SortHeader column="toolCallCount" align="right" sort={sort} onSort={handleSort} label={t('activity.table.toolCalls')} />
            <SortHeader column="totalTokens" align="right" sort={sort} onSort={handleSort} label={t('activity.table.tokens')} />
            <SortHeader column="durationMs" align="right" sort={sort} onSort={handleSort} label={t('activity.table.duration')} />
            <SortHeader column="startedAt" align="right" sort={sort} onSort={handleSort} label={t('activity.table.startedAt')} />
          </div>
        </div>
        <div role="rowgroup">
          {sorted.map((item) => (
            <TraceTableRow key={item.traceId} item={item} onOpen={onOpen} />
          ))}
        </div>
      </div>
    </div>
  )
}

function SortHeader({
  column,
  sort,
  onSort,
  label,
  align = 'left',
}: {
  column: TraceSortKey
  sort: TraceSort
  onSort: (key: TraceSortKey) => void
  label: string
  align?: 'left' | 'right'
}) {
  const { t } = useTranslation()
  const active = sort.key === column
  const Icon = !active ? ArrowUpDown : sort.direction === 'asc' ? ArrowUp : ArrowDown
  return (
    <span
      role="columnheader"
      aria-sort={active ? (sort.direction === 'asc' ? 'ascending' : 'descending') : 'none'}
      className={align === 'right' ? 'text-right' : undefined}
    >
      <button
        type="button"
        data-sort-key={column}
        aria-label={t('activity.table.sortBy', { column: label })}
        onClick={() => onSort(column)}
        className={`inline-flex items-center gap-1 text-[11px] font-medium transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-clay/35 ${
          active ? 'text-ink' : 'text-ink-faint hover:text-ink-soft'
        }`}
      >
        {label}
        <Icon size={11} aria-hidden="true" className={active ? '' : 'opacity-50'} />
      </button>
    </span>
  )
}

function TraceTableRow({
  item,
  onOpen,
}: {
  item: TraceListItem
  onOpen: (item: TraceListItem) => void
}) {
  const { t } = useTranslation()
  // 没有 Turn 的 Trace 是一次独立压缩：不显示并不存在的调用计数，
  // 但 token 是 span 实测合计，照常显示；行仍可通过 trace_id 打开同一个详情抽屉。
  const isTurnless = item.turnId == null
  return (
    <button
      type="button"
      role="row"
      data-trace-row={item.traceId}
      data-model-calls={isTurnless ? undefined : item.modelSubmissionCount}
      data-tool-calls={isTurnless ? undefined : item.toolCallCount}
      data-total-tokens={item.totalTokens}
      onClick={() => onOpen(item)}
      className={`grid ${TABLE_GRID} w-full items-center gap-3 px-4 py-2.5 text-left transition-colors hover:bg-paper-hover focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-clay/35 [&+&]:border-t [&+&]:border-line`}
    >
      <span role="cell"><StatusBadge status={item.status} /></span>
      <span role="cell" className="min-w-0">
        <span className="flex items-center gap-2">
          <span className="truncate text-sm font-medium text-ink">{item.title}</span>
          {isTurnless ? (
            <span className="inline-flex shrink-0 items-center gap-1 rounded-full bg-paper-hover px-2 py-0.5 text-[10px] text-ink-soft">
              <Minimize2 size={10} />
              {t('activity.compaction')}
            </span>
          ) : null}
        </span>
        <span className="mt-0.5 block truncate font-mono text-[11px] text-ink-faint">
          {item.workingDirectory || item.sessionId}
        </span>
      </span>
      <span role="cell" className="truncate text-xs text-ink-soft">
        {item.resolvedModelName || '—'}
      </span>
      <NumericCell value={isTurnless ? null : item.modelSubmissionCount} />
      <NumericCell value={isTurnless ? null : item.toolCallCount} />
      <NumericCell value={item.totalTokens} />
      <span role="cell" className="text-right font-mono text-xs tabular-nums text-ink-soft">
        {formatDuration(item.durationMs)}
      </span>
      <span role="cell" className="text-right text-[11px] text-ink-faint">
        {formatBeijingDateTime(item.startedAt)}
      </span>
    </button>
  )
}

function NumericCell({ value }: { value: number | null }) {
  return (
    <span role="cell" className="text-right font-mono text-xs tabular-nums text-ink-soft">
      {value == null ? '—' : value}
    </span>
  )
}

function StatusBadge({ status }: { status: string }) {
  const { t } = useTranslation()
  return (
    <span className={`inline-flex items-center gap-1.5 rounded-full px-2 py-0.5 text-[10px] font-semibold ${statusBadge(status)}`}>
      <span className={`size-1.5 rounded-full ${statusDot(status)}`} />
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
