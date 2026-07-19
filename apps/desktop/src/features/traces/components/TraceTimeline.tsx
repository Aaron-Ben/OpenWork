import { Bot, Wrench } from 'lucide-react'
import { useMemo } from 'react'
import { useTranslation } from 'react-i18next'

import type { RuntimeTraceSpan } from '../../../bridge/compat'
import {
  buildTraceTree,
  buildWaterfallRows,
  type WaterfallRow,
} from '../traceViewModel'
import { formatDuration } from './TraceList'

interface TraceTimelineProps {
  spans: RuntimeTraceSpan[]
  selectedSpanId: string | null
  onSelect: (span: RuntimeTraceSpan) => void
}

export function TraceTimeline({ spans, selectedSpanId, onSelect }: TraceTimelineProps) {
  const { t } = useTranslation()
  const tree = useMemo(() => buildTraceTree(spans), [spans])
  const waterfall = useMemo(() => buildWaterfallRows(spans), [spans])
  const rowById = useMemo(
    () => new Map(waterfall.map((row) => [row.span.id, row])),
    [waterfall],
  )

  return (
    <div data-trace-waterfall="true" role="list" aria-label={t('activity.timeline')} className="grid gap-3">
      {tree.models.map((node) => (
        <div key={node.span.id} className="grid gap-px">
          <TraceRow
            span={node.span}
            row={rowById.get(node.span.id)}
            selected={selectedSpanId === node.span.id}
            onSelect={onSelect}
          />
          {node.children.length > 0 ? (
            <div className="ml-[11px] grid gap-px border-l border-line pl-2">
              {node.children.map((child) => (
                <TraceRow
                  key={child.id}
                  span={child}
                  row={rowById.get(child.id)}
                  selected={selectedSpanId === child.id}
                  onSelect={onSelect}
                />
              ))}
            </div>
          ) : null}
        </div>
      ))}
      {tree.orphans.length > 0 ? (
        <div className="grid gap-px">
          {tree.orphans.map((span) => (
            <TraceRow
              key={span.id}
              span={span}
              row={rowById.get(span.id)}
              selected={selectedSpanId === span.id}
              onSelect={onSelect}
            />
          ))}
          <p className="mt-1 text-xs text-status-warning-ink">
            {t('activity.orphanTools', { count: tree.orphans.length })}
          </p>
        </div>
      ) : null}
    </div>
  )
}

function TraceRow({
  span,
  row,
  selected,
  onSelect,
}: {
  span: RuntimeTraceSpan
  row: WaterfallRow | undefined
  selected: boolean
  onSelect: (span: RuntimeTraceSpan) => void
}) {
  const { t } = useTranslation()
  return (
    <button
      type="button"
      role="listitem"
      data-span-id={span.id}
      aria-pressed={selected}
      onClick={() => onSelect(span)}
      className={`flex w-full items-center gap-2 rounded-md px-2 py-1.5 text-left transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-clay/35 ${
        selected ? 'bg-clay-soft' : 'hover:bg-paper-hover'
      }`}
    >
      <span className={`size-1.5 shrink-0 rounded-full ${spanStatusDot(span.status)}`} />
      {span.kind === 'model_call' ? (
        <Bot size={13} className="shrink-0 text-ink-faint" />
      ) : (
        <Wrench size={13} className="shrink-0 text-ink-faint" />
      )}
      <span
        className={`min-w-0 flex-1 truncate text-xs ${
          selected ? 'font-medium text-ink' : 'text-ink-soft'
        }`}
      >
        {span.kind === 'model_call'
          ? span.resolvedModelName ?? t('activity.modelCall')
          : span.resolvedToolName ?? span.requestedToolName ?? t('activity.toolCall')}
      </span>
      <span className="w-12 shrink-0 text-right font-mono text-[11px] tabular-nums text-ink-faint">
        {row ? formatDuration(row.durationMs) : '—'}
      </span>
      {row ? (
        <span className="h-1 w-16 shrink-0 overflow-hidden rounded-full bg-paper-hover">
          <span
            className={`block h-full rounded-full ${
              span.kind === 'model_call' ? 'bg-trace-bar-model' : 'bg-trace-bar-tool'
            }`}
            style={{ marginLeft: `${row.leftPercent}%`, width: `${row.widthPercent}%` }}
          />
        </span>
      ) : (
        <span className="w-16 shrink-0" aria-hidden="true" />
      )}
    </button>
  )
}

function spanStatusDot(status: string): string {
  if (status === 'succeeded' || status === 'completed') return 'bg-status-success'
  if (status === 'running') return 'bg-status-warning'
  if (status === 'cancelled' || status === 'interrupted') return 'bg-ink-faint'
  return 'bg-status-danger'
}
