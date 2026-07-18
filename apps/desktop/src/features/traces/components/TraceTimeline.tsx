import { Bot, Wrench } from 'lucide-react'
import { useMemo } from 'react'
import { useTranslation } from 'react-i18next'

import type { RuntimeTraceSpan } from '../../../bridge/compat'
import { buildTraceTree, buildWaterfallRows } from '../traceViewModel'
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
  const rowById = new Map(waterfall.map((row) => [row.span.id, row]))
  const ordered = [
    ...tree.models.flatMap((node) => [node.span, ...node.children]),
    ...tree.orphans,
  ]

  return (
    <div data-trace-waterfall="true" className="grid gap-2" role="list" aria-label={t('activity.timeline')}>
      {ordered.map((span) => {
        const row = rowById.get(span.id)
        const child = span.kind === 'tool_call' && Boolean(span.parentSpanId)
        return (
          <button
            key={span.id}
            type="button"
            role="listitem"
            data-span-id={span.id}
            aria-pressed={selectedSpanId === span.id}
            className={`rounded-xl border p-3 text-left transition focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-clay/35 ${
              selectedSpanId === span.id
                ? 'border-clay bg-clay-soft'
                : 'border-line bg-surface hover:border-clay/40'
            } ${child ? 'ml-5' : ''}`}
            onClick={() => onSelect(span)}
          >
            <span className="flex items-center gap-2">
              {span.kind === 'model_call' ? <Bot size={15} /> : <Wrench size={15} />}
              <span className="min-w-0 flex-1 truncate text-xs font-semibold text-ink">
                {span.kind === 'model_call'
                  ? span.resolvedModelName ?? t('activity.modelCall')
                  : span.resolvedToolName ?? span.requestedToolName ?? t('activity.toolCall')}
              </span>
              <span className="text-[10px] text-ink-faint">{row ? formatDuration(row.durationMs) : '—'}</span>
            </span>
            {row ? (
              <span className="mt-2 block h-1.5 overflow-hidden rounded-full bg-paper-hover">
                <span
                  className={`block h-full rounded-full ${span.kind === 'model_call' ? 'bg-trace-bar-model' : 'bg-trace-bar-tool'}`}
                  style={{ marginLeft: `${row.leftPercent}%`, width: `${row.widthPercent}%` }}
                />
              </span>
            ) : null}
          </button>
        )
      })}
      {tree.orphans.length > 0 ? (
        <p className="mt-1 text-xs text-status-warning-ink">{t('activity.orphanTools', { count: tree.orphans.length })}</p>
      ) : null}
    </div>
  )
}
