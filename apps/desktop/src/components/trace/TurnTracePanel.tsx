import { useEffect, useMemo, useRef, useState } from 'react'
import {
  Activity,
  AlertCircle,
  Bot,
  ChevronRight,
  Clock3,
  Maximize2,
  Network,
  RotateCcw,
  ShieldCheck,
  Workflow,
  Wrench,
  X,
  ZoomIn,
  ZoomOut,
} from 'lucide-react'
import { motion } from 'motion/react'
import { useTranslation } from 'react-i18next'

import { sessionsApi } from '../../api/sessions'
import type { TraceSpan, TraceSpanDetailView, TraceSpanKind, TurnTrace } from '../../type/trace'
import { resolveErrorMessage } from '../../utils/commandError'
import { TraceSpanDetailPanel } from './TraceSpanDetail'
import {
  buildTraceTree,
  buildWaterfallTicks,
  calculateWaterfallSegment,
  filterVisibleTraceRows,
  flattenTraceTree,
  formatDuration,
  type FlatTraceTreeNode,
} from './traceViewModel'

export type TraceViewMode = 'tree' | 'waterfall'

export const DEFAULT_TRACE_VIEW_MODE: TraceViewMode = 'tree'

const WATERFALL_MIN_ZOOM = 1
const WATERFALL_MAX_ZOOM = 3
const WATERFALL_ZOOM_STEP = 0.5

const TRACE_BAR_COLOR: Record<TraceSpanKind, string> = {
  turn: 'bg-trace-bar-turn',
  step: 'bg-trace-bar-step',
  model_attempt: 'bg-trace-bar-model',
  transport_attempt: 'bg-trace-bar-transport',
  tool_run: 'bg-trace-bar-tool',
  approval: 'bg-trace-bar-approval',
  recovery: 'bg-trace-bar-recovery',
}

interface TurnTracePanelProps {
  turnId: string
  onClose: () => void
  initialSpanId?: string
  initialProviderToolCallId?: string
  onRevealMessage?: (messageId: string) => void
  onRevealTool?: (providerToolCallId: string) => void
}

export function TurnTracePanel({
  turnId,
  onClose,
  initialSpanId,
  initialProviderToolCallId,
  onRevealMessage,
  onRevealTool,
}: TurnTracePanelProps) {
  const { i18n, t } = useTranslation()
  const [trace, setTrace] = useState<TurnTrace | null>(null)
  const [selectedSpanId, setSelectedSpanId] = useState<string | null>(null)
  const [detail, setDetail] = useState<TraceSpanDetailView | null>(null)
  const detailCache = useRef(new Map<string, TraceSpanDetailView>())
  const [error, setError] = useState<string | null>(null)
  const [detailError, setDetailError] = useState<string | null>(null)
  const [isDetailLoading, setIsDetailLoading] = useState(false)
  const [treePercent, setTreePercent] = useState(55)
  const [viewMode, setViewMode] = useState<TraceViewMode>(DEFAULT_TRACE_VIEW_MODE)
  const [waterfallZoom, setWaterfallZoom] = useState(WATERFALL_MIN_ZOOM)
  const [collapsedSpanIds, setCollapsedSpanIds] = useState<Set<string>>(() => new Set())
  const splitRef = useRef<HTMLDivElement>(null)

  useEffect(() => {
    let active = true
    setTrace(null)
    setSelectedSpanId(null)
    setDetail(null)
    setError(null)
    setViewMode(DEFAULT_TRACE_VIEW_MODE)
    setWaterfallZoom(WATERFALL_MIN_ZOOM)
    setCollapsedSpanIds(new Set())
    detailCache.current.clear()
    void sessionsApi
      .traceTurn(turnId)
      .then((value) => {
        if (!active) return
        setTrace(value)
        const requested = initialSpanId
          ? value.spans.find((span) => span.spanId === initialSpanId)
          : initialProviderToolCallId
            ? value.spans.find(
                (span) => span.attributes.providerToolCallId === initialProviderToolCallId,
              )
            : null
        setSelectedSpanId(
          requested?.spanId
            ?? value.summary.diagnosis.focusSpanId
            ?? value.spans.find((span) => span.spanKind === 'turn')?.spanId
            ?? value.spans[0]?.spanId
            ?? null,
        )
      })
      .catch((reason) => active && setError(resolveErrorMessage(reason)))
    return () => {
      active = false
    }
  }, [initialProviderToolCallId, initialSpanId, turnId])

  useEffect(() => {
    if (!selectedSpanId) return
    const cached = detailCache.current.get(selectedSpanId)
    if (cached) {
      setDetail(cached)
      setDetailError(null)
      return
    }
    let active = true
    setDetail(null)
    setDetailError(null)
    setIsDetailLoading(true)
    void sessionsApi
      .traceSpanDetail(turnId, selectedSpanId)
      .then((value) => {
        if (!active) return
        if (!['running', 'waiting'].includes(value.span.status)) {
          detailCache.current.set(selectedSpanId, value)
        }
        setDetail(value)
      })
      .catch((reason) => active && setDetailError(resolveErrorMessage(reason)))
      .finally(() => active && setIsDetailLoading(false))
    return () => {
      active = false
    }
  }, [selectedSpanId, turnId])

  const rows = useMemo(
    () => flattenTraceTree(buildTraceTree(trace?.spans ?? [])),
    [trace],
  )

  function beginResize(event: React.PointerEvent<HTMLDivElement>) {
    event.preventDefault()
    const move = (moveEvent: PointerEvent) => {
      const rect = splitRef.current?.getBoundingClientRect()
      if (!rect) return
      const percentage = ((moveEvent.clientX - rect.left) / rect.width) * 100
      setTreePercent(Math.min(72, Math.max(32, percentage)))
    }
    const stop = () => {
      window.removeEventListener('pointermove', move)
      window.removeEventListener('pointerup', stop)
    }
    window.addEventListener('pointermove', move)
    window.addEventListener('pointerup', stop)
  }

  function toggleCollapsedSpan(spanId: string) {
    setCollapsedSpanIds((current) => {
      const next = new Set(current)
      if (next.has(spanId)) next.delete(spanId)
      else next.add(spanId)
      return next
    })
  }

  return (
    <motion.aside
      initial={{ opacity: 0, x: 24 }}
      animate={{ opacity: 1, x: 0 }}
      exit={{ opacity: 0, x: 24 }}
      transition={{ duration: 0.2, ease: 'easeOut' }}
      className="absolute inset-y-0 right-0 z-30 flex w-[min(1040px,97vw)] flex-col border-l border-line bg-paper shadow-[-18px_0_45px_rgba(20,20,19,0.08)]"
      aria-label={t('trace.title')}
    >
      <header className="flex min-h-[72px] shrink-0 items-start gap-3 border-b border-line px-4 py-3">
        <div className="min-w-0 flex-1">
          <div className="flex min-w-0 items-center gap-2">
            <div className="shrink-0 font-sans text-sm font-semibold text-ink">{t('trace.title')}</div>
            {trace ? (
              <div className="truncate text-xs text-ink-faint">
                {trace.summary.inputPreview ?? trace.summary.sessionTitle ?? trace.summary.turnId}
              </div>
            ) : null}
          </div>
          {trace ? (
            <div className="mt-1.5 flex flex-wrap items-center gap-x-3 gap-y-1 text-[11px] text-ink-faint">
              <span className="flex items-center gap-1.5 font-medium text-ink-soft">
                <span className={`size-1.5 rounded-full ${statusColor(trace.summary.status)}`} />
                {t(`settings.trace.status.${trace.summary.status}`)}
              </span>
              <span>{trace.summary.model ?? t('trace.unknownModel')}</span>
              <span className="flex items-center gap-1 tabular-nums">
                <Clock3 size={12} />
                {formatDuration(trace.summary.durationMs, i18n.language)}
              </span>
              <span>{t('settings.trace.tokens', { count: trace.summary.inputTokens + trace.summary.outputTokens })}</span>
              <span>{t('trace.spans', { count: trace.spans.length })}</span>
            </div>
          ) : null}
        </div>
        <button
          type="button"
          onClick={onClose}
          aria-label={t('trace.close')}
          className="rounded-lg p-1.5 text-ink-faint hover:bg-paper-hover hover:text-ink"
        >
          <X size={17} />
        </button>
      </header>

      {!trace && !error ? (
        <p className="p-5 font-sans text-sm text-ink-faint">{t('trace.loading')}</p>
      ) : null}
      {error ? <p className="m-4 rounded-xl bg-clay-soft p-3 text-sm text-ink">{error}</p> : null}

      {trace ? (
        <>
          <button
            type="button"
            onClick={() => trace.summary.diagnosis.focusSpanId && setSelectedSpanId(trace.summary.diagnosis.focusSpanId)}
            className={`mx-4 mt-4 flex items-center gap-3 rounded-xl border p-3 text-left ${diagnosisStyle(trace.summary.diagnosis.status)}`}
          >
            <AlertCircle size={17} className="shrink-0" />
            <span className="min-w-0 flex-1">
              <span className="block text-xs font-semibold">
                {t(`trace.diagnosis.${trace.summary.diagnosis.reason}`)}
              </span>
              <span className="mt-0.5 block text-[11px] opacity-75">
                {t(`trace.completeness.${trace.summary.dataCompleteness}`)}
              </span>
            </span>
            <span className="flex shrink-0 items-center gap-1 text-xs tabular-nums">
              <Clock3 size={13} />
              {formatDuration(trace.summary.durationMs, i18n.language)}
            </span>
          </button>

          <div
            ref={splitRef}
            className="mt-4 grid min-h-0 flex-1 overflow-hidden border-t border-line max-[760px]:!grid-cols-1 max-[760px]:overflow-auto"
            style={{ gridTemplateColumns: `${treePercent}% 5px minmax(0, 1fr)` }}
          >
            <div className="min-w-0 overflow-auto max-[760px]:overflow-visible">
              <TraceSpanNavigator
                rows={rows}
                viewMode={viewMode}
                selectedSpanId={selectedSpanId}
                turnStartedAt={trace.summary.startedAt}
                turnDurationMs={trace.summary.durationMs}
                waterfallZoom={waterfallZoom}
                collapsedSpanIds={collapsedSpanIds}
                onViewModeChange={setViewMode}
                onWaterfallZoomChange={setWaterfallZoom}
                onToggleCollapse={toggleCollapsedSpan}
                onSelect={setSelectedSpanId}
              />
            </div>

            <div
              role="separator"
              aria-orientation="vertical"
              aria-label={t('trace.resize')}
              onPointerDown={beginResize}
              className="cursor-col-resize bg-line hover:bg-clay max-[760px]:hidden"
            />

            <div className="min-w-0 overflow-auto border-l border-line max-[760px]:border-l-0 max-[760px]:border-t">
              {isDetailLoading ? (
                <p className="p-4 text-sm text-ink-faint">{t('trace.detail.loading')}</p>
              ) : null}
              {detailError ? <p className="m-4 rounded-xl bg-clay-soft p-3 text-sm">{detailError}</p> : null}
              {detail ? (
                <TraceSpanDetailPanel
                  key={detail.span.spanId}
                  value={detail}
                  onRevealMessage={onRevealMessage}
                  onRevealTool={onRevealTool}
                />
              ) : !isDetailLoading && !detailError ? (
                <p className="p-4 text-sm text-ink-faint">{t('trace.detail.selectSpan')}</p>
              ) : null}
            </div>
          </div>
        </>
      ) : null}
    </motion.aside>
  )
}

export function TraceSpanNavigator({
  rows,
  viewMode,
  selectedSpanId,
  turnStartedAt,
  turnDurationMs,
  waterfallZoom,
  collapsedSpanIds,
  onViewModeChange,
  onWaterfallZoomChange,
  onToggleCollapse,
  onSelect,
}: {
  rows: FlatTraceTreeNode[]
  viewMode: TraceViewMode
  selectedSpanId: string | null
  turnStartedAt: number
  turnDurationMs: number
  waterfallZoom: number
  collapsedSpanIds: ReadonlySet<string>
  onViewModeChange: (viewMode: TraceViewMode) => void
  onWaterfallZoomChange: (zoom: number) => void
  onToggleCollapse: (spanId: string) => void
  onSelect: (spanId: string) => void
}) {
  const { t } = useTranslation()
  const visibleTreeRows = filterVisibleTraceRows(rows, collapsedSpanIds)
  return (
    <section data-trace-view-mode={viewMode}>
      <div className="sticky top-0 z-10 flex min-h-11 items-center justify-between gap-3 border-b border-line bg-paper px-3 py-2">
        <span className="text-[10px] uppercase tracking-wide text-ink-faint">
          {t('trace.spans', { count: rows.length })}
        </span>
        <div className="flex items-center gap-1.5">
          <div
            role="tablist"
            aria-label={t('trace.viewSelector')}
            className="flex items-center rounded-lg border border-line bg-paper-hover p-0.5 text-xs"
          >
            {(['tree', 'waterfall'] as const).map((mode) => (
              <button
                key={mode}
                type="button"
                role="tab"
                data-trace-view-tab={mode}
                aria-selected={viewMode === mode}
                onClick={() => onViewModeChange(mode)}
                className={`rounded-md px-2.5 py-1 transition-colors ${
                  viewMode === mode
                    ? 'bg-paper text-ink shadow-sm'
                    : 'text-ink-faint hover:text-ink'
                }`}
              >
                {t(`trace.${mode}`)}
              </button>
            ))}
          </div>
          {viewMode === 'waterfall' ? (
            <WaterfallZoomControls
              zoom={waterfallZoom}
              onZoomChange={onWaterfallZoomChange}
            />
          ) : null}
        </div>
      </div>

      {viewMode === 'tree' ? (
        <div role="tree" aria-label={t('trace.tree')} className="p-2">
          {visibleTreeRows.map((node) => (
            <TraceTreeRow
              key={node.span.spanId}
              span={node.span}
              depth={node.depth}
              hasChildren={node.children.length > 0}
              collapsed={collapsedSpanIds.has(node.span.spanId)}
              selected={node.span.spanId === selectedSpanId}
              onToggleCollapse={onToggleCollapse}
              onSelect={onSelect}
            />
          ))}
        </div>
      ) : (
        <WaterfallCanvas
          rows={rows}
          selectedSpanId={selectedSpanId}
          turnStartedAt={turnStartedAt}
          turnDurationMs={turnDurationMs}
          zoom={waterfallZoom}
          onSelect={onSelect}
        />
      )}
    </section>
  )
}

function TraceTreeRow({
  span,
  depth,
  hasChildren,
  collapsed,
  selected,
  onToggleCollapse,
  onSelect,
}: {
  span: TraceSpan
  depth: number
  hasChildren: boolean
  collapsed: boolean
  selected: boolean
  onToggleCollapse: (spanId: string) => void
  onSelect: (spanId: string) => void
}) {
  const { t } = useTranslation()
  return (
    <div
      role="treeitem"
      aria-level={depth + 1}
      aria-expanded={hasChildren ? !collapsed : undefined}
      aria-selected={selected}
      data-trace-tree-row={span.spanId}
      data-selected={selected ? 'true' : 'false'}
      className={`flex min-h-9 w-full items-center rounded-lg px-1 text-xs ${
        selected ? 'bg-clay-soft' : 'hover:bg-paper-hover'
      }`}
      style={{ paddingLeft: Math.min(depth, 5) * 12 + 4 }}
    >
      {hasChildren ? (
        <button
          type="button"
          data-trace-collapse-toggle={span.spanId}
          aria-label={t(collapsed ? 'trace.expand' : 'trace.collapse')}
          aria-expanded={!collapsed}
          onClick={() => onToggleCollapse(span.spanId)}
          className="grid size-5 shrink-0 place-items-center rounded text-ink-faint hover:bg-paper hover:text-ink"
        >
          <ChevronRight
            size={13}
            className={`transition-transform ${collapsed ? '' : 'rotate-90'}`}
          />
        </button>
      ) : (
        <span aria-hidden="true" className="size-5 shrink-0" />
      )}
      <button
        type="button"
        data-trace-span-row={span.spanId}
        onClick={() => onSelect(span.spanId)}
        className="flex min-w-0 flex-1 items-center px-1 py-2 text-left"
      >
        <SpanInlineLabel span={span} depth={0} />
      </button>
    </div>
  )
}

function WaterfallZoomControls({
  zoom,
  onZoomChange,
}: {
  zoom: number
  onZoomChange: (zoom: number) => void
}) {
  const { t } = useTranslation()
  const controls = [
    {
      id: 'out',
      label: t('trace.zoomOut'),
      icon: ZoomOut,
      disabled: zoom <= WATERFALL_MIN_ZOOM,
      nextZoom: zoom - WATERFALL_ZOOM_STEP,
    },
    {
      id: 'in',
      label: t('trace.zoomIn'),
      icon: ZoomIn,
      disabled: zoom >= WATERFALL_MAX_ZOOM,
      nextZoom: zoom + WATERFALL_ZOOM_STEP,
    },
    {
      id: 'reset',
      label: t('trace.resetZoom'),
      icon: Maximize2,
      disabled: zoom === WATERFALL_MIN_ZOOM,
      nextZoom: WATERFALL_MIN_ZOOM,
    },
  ] as const

  return (
    <div
      role="group"
      aria-label={t('trace.waterfallZoomControls')}
      className="flex items-center rounded-lg border border-line bg-paper-hover p-0.5"
    >
      {controls.map((control) => {
        const Icon = control.icon
        return (
          <button
            key={control.id}
            type="button"
            data-waterfall-zoom-control={control.id}
            aria-label={control.label}
            title={control.label}
            disabled={control.disabled}
            onClick={() => onZoomChange(clampWaterfallZoom(control.nextZoom))}
            className="grid size-6 place-items-center rounded-md text-ink-faint hover:bg-paper hover:text-ink disabled:cursor-default disabled:opacity-35 disabled:hover:bg-transparent"
          >
            <Icon size={13} />
          </button>
        )
      })}
    </div>
  )
}

function WaterfallCanvas({
  rows,
  selectedSpanId,
  turnStartedAt,
  turnDurationMs,
  zoom,
  onSelect,
}: {
  rows: FlatTraceTreeNode[]
  selectedSpanId: string | null
  turnStartedAt: number
  turnDurationMs: number
  zoom: number
  onSelect: (spanId: string) => void
}) {
  const ticks = buildWaterfallTicks(turnDurationMs, zoom)
  return (
    <div
      data-waterfall-scroll-region="true"
      className="min-w-0 overflow-x-auto overscroll-x-contain"
    >
      <div
        data-waterfall-canvas="true"
        className="min-w-full px-3"
        style={{ width: `${zoom * 100}%` }}
      >
        <WaterfallAxis ticks={ticks} />
        <div className="relative py-1">
          <WaterfallGrid ticks={ticks} />
          <div className="relative z-[1]">
            {rows.map((node) => (
              <TraceWaterfallRow
                key={node.span.spanId}
                span={node.span}
                depth={node.depth}
                selected={node.span.spanId === selectedSpanId}
                turnStartedAt={turnStartedAt}
                turnDurationMs={turnDurationMs}
                onSelect={onSelect}
              />
            ))}
          </div>
        </div>
      </div>
    </div>
  )
}

function TraceWaterfallRow({
  span,
  depth,
  selected,
  turnStartedAt,
  turnDurationMs,
  onSelect,
}: {
  span: TraceSpan
  depth: number
  selected: boolean
  turnStartedAt: number
  turnDurationMs: number
  onSelect: (spanId: string) => void
}) {
  const { i18n, t } = useTranslation()
  const segment = calculateWaterfallSegment(span, turnStartedAt, turnDurationMs)
  const title = spanTitle(span, t)
  const duration = span.durationMs == null ? '—' : formatDuration(span.durationMs, i18n.language)
  return (
    <button
      type="button"
      data-trace-waterfall-row={span.spanId}
      data-trace-span-row={span.spanId}
      data-selected={selected ? 'true' : 'false'}
      aria-label={`${title} · ${duration}`}
      onClick={() => onSelect(span.spanId)}
      className={`relative block h-9 w-full rounded-md text-left text-[11px] ${
        selected ? 'bg-clay-soft/70' : 'hover:bg-paper-hover/70'
      }`}
    >
      <span
        data-waterfall-bar={span.spanId}
        className={`absolute top-1/2 h-6 min-w-1 -translate-y-1/2 rounded-md border border-trace-bar-outline text-trace-bar-ink shadow-sm transition-[filter,box-shadow] hover:brightness-95 ${barColor(span)} ${
          selected ? 'ring-2 ring-clay ring-offset-1 ring-offset-paper' : ''
        }`}
        style={{ left: `${segment.leftPercent}%`, width: `${segment.widthPercent}%` }}
      >
        <span
          data-waterfall-bar-label="true"
          className="pointer-events-none flex h-full min-w-max items-center gap-1 whitespace-nowrap pr-2 font-medium"
          style={{ paddingLeft: Math.min(depth, 5) * 5 + 7 }}
        >
          <SpanIcon span={span} tone="bar" />
          <span>{title}</span>
          <span className="font-normal opacity-80">· {duration}</span>
        </span>
      </span>
    </button>
  )
}

function SpanInlineLabel({
  span,
  depth,
}: {
  span: TraceSpan
  depth: number
}) {
  const { i18n, t } = useTranslation()
  return (
    <span
      data-trace-span-label="true"
      className="flex min-w-0 items-center gap-1.5"
      style={{ paddingLeft: Math.min(depth, 5) * 12 }}
    >
      <SpanIcon span={span} />
      <span className="min-w-0 truncate text-ink">{spanTitle(span, t)}</span>
      <span
        data-trace-inline-duration="true"
        className="shrink-0 tabular-nums text-[11px] text-ink-faint"
      >
        {span.durationMs == null ? '—' : formatDuration(span.durationMs, i18n.language)}
      </span>
      <span className={`size-1.5 shrink-0 rounded-full ${statusColor(span.status)}`} />
    </span>
  )
}

function WaterfallAxis({
  ticks,
}: {
  ticks: ReturnType<typeof buildWaterfallTicks>
}) {
  const { i18n } = useTranslation()
  return (
    <div
      data-trace-waterfall-axis="true"
      className="relative h-8 border-b border-line text-[9px] tabular-nums text-ink-faint"
    >
      {ticks.map((tick) => (
        <span
          key={tick.valueMs}
          data-waterfall-tick={tick.valueMs}
          className="absolute bottom-1 whitespace-nowrap"
          style={{
            left: `${tick.leftPercent}%`,
            transform: tick.leftPercent === 0 ? undefined : 'translateX(-50%)',
          }}
        >
          {formatDuration(tick.valueMs, i18n.language)}
        </span>
      ))}
    </div>
  )
}

function WaterfallGrid({
  ticks,
}: {
  ticks: ReturnType<typeof buildWaterfallTicks>
}) {
  return (
    <>
      {ticks.map((tick) => (
        <span
          key={tick.valueMs}
          aria-hidden="true"
          className="absolute inset-y-0 border-l border-line/60"
          style={{ left: `${tick.leftPercent}%` }}
        />
      ))}
    </>
  )
}

function SpanIcon({
  span,
  tone = 'muted',
}: {
  span: TraceSpan
  tone?: 'muted' | 'bar'
}) {
  const className = tone === 'bar' ? 'shrink-0 text-current opacity-80' : 'shrink-0 text-ink-faint'
  if (span.spanKind === 'tool_run') return <Wrench size={13} className={className} />
  if (span.spanKind === 'approval') return <ShieldCheck size={13} className={className} />
  if (span.spanKind === 'model_attempt') return <Bot size={13} className={className} />
  if (span.spanKind === 'transport_attempt') return <Network size={13} className={className} />
  if (span.spanKind === 'step') return <Workflow size={13} className={className} />
  if (span.spanKind === 'recovery') return <RotateCcw size={13} className={className} />
  return <Activity size={13} className={className} />
}

function spanTitle(
  span: TraceSpan,
  t: (key: string, options?: Record<string, unknown>) => string,
): string {
  const toolName = typeof span.attributes.toolName === 'string' ? span.attributes.toolName : null
  if (span.spanKind === 'tool_run' && toolName) return t('trace.toolRun', { name: toolName })
  const attempt = typeof span.attributes.transportAttempt === 'number'
    ? span.attributes.transportAttempt
    : null
  if (span.spanKind === 'transport_attempt' && attempt != null) {
    return `${t(`trace.kind.${span.spanKind}`)} #${attempt}`
  }
  return t(`trace.kind.${span.spanKind}`)
}

function statusColor(status: TraceSpan['status']): string {
  if (status === 'failed' || status === 'denied') return 'bg-status-danger'
  if (status === 'running' || status === 'waiting') return 'bg-status-warning'
  if (status === 'succeeded') return 'bg-status-success'
  return 'bg-ink-faint'
}

function barColor(span: TraceSpan): string {
  if (span.status === 'failed' || span.status === 'denied') return 'bg-trace-bar-error'
  return TRACE_BAR_COLOR[span.spanKind] ?? 'bg-trace-bar-other'
}

function clampWaterfallZoom(value: number): number {
  const stepped = Math.round(value / WATERFALL_ZOOM_STEP) * WATERFALL_ZOOM_STEP
  return Math.min(WATERFALL_MAX_ZOOM, Math.max(WATERFALL_MIN_ZOOM, stepped))
}

function diagnosisStyle(status: 'healthy' | 'attention' | 'blocked'): string {
  if (status === 'blocked') {
    return 'border-status-danger-border bg-status-danger-soft text-status-danger-ink'
  }
  if (status === 'attention') {
    return 'border-status-warning-border bg-status-warning-soft text-status-warning-ink'
  }
  return 'border-status-success-border bg-status-success-soft text-status-success-ink'
}
