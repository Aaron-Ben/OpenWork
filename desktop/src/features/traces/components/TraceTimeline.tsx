import { Bot, ChevronRight, Minimize2, ShieldCheck, ShieldX, UserCheck, UserX } from 'lucide-react'
import { AnimatePresence, motion } from 'motion/react'
import { useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'

import type { RuntimeTraceSpan } from '@/bridge/compat'
import {
  buildTraceTree,
  buildWaterfallRange,
  buildWaterfallRows,
  type TraceModelNode,
  type WaterfallRange,
  type WaterfallRow,
} from '../traceViewModel'
import { formatDuration } from './TraceList'
import { TraceToolIcon } from './traceToolIcons'

// 名称列 / 耗时列 / 瀑布轨道列共享同一栅格模板，刻度尺与所有数据行因此严格对齐。
const TIMELINE_GRID = 'grid-cols-[32px_minmax(0,1fr)_56px_minmax(96px,42%)]'
const RULER_FRACTIONS = [0, 0.25, 0.5, 0.75, 1] as const

interface TraceTimelineProps {
  spans: RuntimeTraceSpan[]
  selectedSpanId: string | null
  onSelect: (span: RuntimeTraceSpan) => void
}

export function TraceTimeline({ spans, selectedSpanId, onSelect }: TraceTimelineProps) {
  const { t } = useTranslation()
  const tree = useMemo(() => buildTraceTree(spans), [spans])
  const waterfall = useMemo(() => buildWaterfallRows(spans), [spans])
  const range = useMemo(() => buildWaterfallRange(spans), [spans])
  const rowById = useMemo(
    () => new Map(waterfall.map((row) => [row.span.id, row])),
    [waterfall],
  )
  // 折叠是纯视图状态：默认全展开（第一层 model_call / compaction 直接可见），
  // 有子 span 的节点可单独把工具调用收起来降噪。
  const [collapsedIds, setCollapsedIds] = useState<ReadonlySet<string>>(() => new Set())

  const toggleCollapse = (spanId: string) => {
    setCollapsedIds((current) => {
      const next = new Set(current)
      if (next.has(spanId)) {
        next.delete(spanId)
      } else {
        next.add(spanId)
      }
      return next
    })
  }

  return (
    <div data-trace-waterfall="true" role="list" aria-label={t('activity.timeline')}>
      <div className="mb-3 flex items-center justify-between gap-3 px-1">
        <h3 className="text-xs font-semibold text-ink-soft">{t('activity.timeline')}</h3>
        <div className="flex items-center gap-3 text-[10px] text-ink-faint" aria-label={t('activity.timelineLegend')}>
          <span className="inline-flex items-center gap-1"><span className="size-2 rounded-sm bg-trace-bar-model" />{t('activity.modelLegend')}</span>
          <span className="inline-flex items-center gap-1"><span className="size-2 rounded-sm bg-trace-bar-tool" />{t('activity.toolLegend')}</span>
        </div>
      </div>
      {range ? <TimeRuler range={range} /> : null}
      <div className="grid gap-3">
        {tree.roots.map((node, index) => (
          <TimelineNode
            key={node.span.id}
            node={node}
            sequence={index + 1}
            collapsed={collapsedIds.has(node.span.id)}
            onToggleCollapse={toggleCollapse}
            rowById={rowById}
            selectedSpanId={selectedSpanId}
            onSelect={onSelect}
          />
        ))}
      </div>
      {tree.orphans.length > 0 ? (
        <div className="mt-2 grid gap-px">
          {tree.orphans.map((span) => (
            <TimelineRow
              key={span.id}
              span={span}
              depth={0}
              row={rowById.get(span.id)}
              selected={selectedSpanId === span.id}
              onSelect={onSelect}
            />
          ))}
          <p className="mt-1 px-2 text-xs text-status-warning-ink">
            {t('activity.orphanTools', { count: tree.orphans.length })}
          </p>
        </div>
      ) : null}
    </div>
  )
}

/** 顶部时间刻度：标注相对 Trace 起点的偏移量，与 Jaeger 瀑布一致。 */
function TimeRuler({ range }: { range: WaterfallRange }) {
  const { t } = useTranslation()
  const total = Math.max(1, range.endMs - range.startMs)
  return (
    <div
      data-trace-ruler="true"
      aria-label={t('activity.timeAxis')}
      className={`grid ${TIMELINE_GRID} gap-2 px-2 pb-1.5 pt-0.5`}
    >
      <span aria-hidden="true" />
      <span aria-hidden="true" />
      <span aria-hidden="true" />
      <span className="relative block h-4 select-none text-[10px] tabular-nums text-ink-faint">
        {RULER_FRACTIONS.map((fraction) => (
          <span
            key={fraction}
            className="absolute top-0 whitespace-nowrap"
            style={{
              left: `${fraction * 100}%`,
              transform: fraction === 0
                ? 'none'
                : fraction === 1
                  ? 'translateX(-100%)'
                  : 'translateX(-50%)',
            }}
          >
            {formatDuration(total * fraction)}
          </span>
        ))}
      </span>
    </div>
  )
}

function TimelineNode({
  node,
  sequence,
  collapsed,
  onToggleCollapse,
  rowById,
  selectedSpanId,
  onSelect,
}: {
  node: TraceModelNode
  sequence: number
  collapsed: boolean
  onToggleCollapse: (spanId: string) => void
  rowById: Map<string, WaterfallRow>
  selectedSpanId: string | null
  onSelect: (span: RuntimeTraceSpan) => void
}) {
  const hasChildren = node.children.length > 0
  const sequenceLabel = String(sequence).padStart(2, '0')
  return (
    <div
      data-trace-group={node.span.id}
      data-trace-sequence={sequenceLabel}
      className="overflow-hidden rounded-xl border border-line bg-paper"
    >
      <TimelineRow
        span={node.span}
        sequence={sequenceLabel}
        depth={0}
        row={rowById.get(node.span.id)}
        selected={selectedSpanId === node.span.id}
        hasChildren={hasChildren}
        collapsed={collapsed}
        onToggleCollapse={onToggleCollapse}
        onSelect={onSelect}
      />
      <AnimatePresence initial={false}>
        {!collapsed && hasChildren ? (
          <motion.div
            key="children"
            initial={{ height: 0, opacity: 0 }}
            animate={{ height: 'auto', opacity: 1 }}
            exit={{ height: 0, opacity: 0 }}
            transition={{ duration: 0.2, ease: [0.32, 0.72, 0, 1] }}
            className="overflow-hidden"
          >
            <div className="grid gap-px">
              {node.children.map((child) => (
                <TimelineRow
                  key={child.id}
                  span={child}
                  depth={1}
                  row={rowById.get(child.id)}
                  selected={selectedSpanId === child.id}
                  onSelect={onSelect}
                />
              ))}
            </div>
          </motion.div>
        ) : null}
      </AnimatePresence>
    </div>
  )
}

function TimelineRow({
  span,
  sequence,
  depth,
  row,
  selected,
  hasChildren = false,
  collapsed = false,
  onToggleCollapse,
  onSelect,
}: {
  span: RuntimeTraceSpan
  sequence?: string
  depth: number
  row: WaterfallRow | undefined
  selected: boolean
  hasChildren?: boolean
  collapsed?: boolean
  onToggleCollapse?: (spanId: string) => void
  onSelect: (span: RuntimeTraceSpan) => void
}) {
  const { t } = useTranslation()
  return (
    <div
      role="listitem"
      data-span-id={span.id}
      onClick={() => onSelect(span)}
      className={`grid ${TIMELINE_GRID} cursor-pointer items-center gap-2 px-3 py-2 transition-colors ${
        selected ? 'bg-clay-soft' : 'hover:bg-paper-hover'
      } ${depth > 0 ? 'border-t border-line/70' : ''}`}
    >
      <span className="font-mono text-[10px] tabular-nums text-ink-faint">{sequence}</span>
      <div
        className="flex min-w-0 items-center gap-1"
        style={depth > 0 ? { paddingLeft: (depth - 1) * 18 } : undefined}
      >
        {hasChildren && onToggleCollapse ? (
          <button
            type="button"
            data-collapse-toggle={span.id}
            aria-expanded={!collapsed}
            aria-label={collapsed ? t('activity.expand') : t('activity.collapse')}
            onClick={(event) => {
              event.stopPropagation()
              onToggleCollapse(span.id)
            }}
            className="grid size-4 shrink-0 place-items-center rounded text-ink-faint transition-colors hover:bg-paper-hover hover:text-ink focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-clay/35"
          >
            <ChevronRight
              size={12}
              className={`transition-transform duration-200 ${collapsed ? '' : 'rotate-90'}`}
            />
          </button>
        ) : (
          <span className="size-4 shrink-0" aria-hidden="true" />
        )}
        <button
          type="button"
          aria-pressed={selected}
          className="flex min-w-0 flex-1 items-center gap-1.5 rounded text-left focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-clay/35"
        >
          <span className={`size-1.5 shrink-0 rounded-full ${spanStatusDot(span.status)}`} />
          <SpanKindIcon span={span} />
          <span className="min-w-0 flex-1">
            <span
              className={`block truncate text-xs ${
                selected ? 'font-medium text-ink' : 'text-ink-soft'
              }`}
            >
              {spanName(span, t)}
            </span>
          </span>
          <PermissionActivityMarker span={span} />
        </button>
      </div>
      <span className="shrink-0 text-right font-mono text-[11px] tabular-nums text-ink-faint">
        {row ? formatDuration(row.durationMs) : '—'}
      </span>
      <span className="relative block h-3.5 overflow-hidden rounded-[3px] bg-paper-hover/50">
        <TrackGridlines />
        {row ? (
          <span
            className={`absolute inset-y-0 rounded-[3px] ${spanBarColor(span.kind)} transition-[left,width] duration-300 ease-out`}
            style={{ left: `${row.leftPercent}%`, width: `${row.widthPercent}%` }}
          />
        ) : null}
      </span>
    </div>
  )
}

function TrackGridlines() {
  return (
    <span aria-hidden="true" className="pointer-events-none absolute inset-0">
      {[25, 50, 75].map((percent) => (
        <span
          key={percent}
          className="absolute inset-y-0 w-px bg-line/80"
          style={{ left: `${percent}%` }}
        />
      ))}
    </span>
  )
}

function SpanKindIcon({ span }: { span: RuntimeTraceSpan }) {
  if (span.kind === 'model_call') return <Bot size={13} className="shrink-0 text-trace-bar-model" />
  if (span.kind === 'compaction') return <Minimize2 size={13} className="shrink-0 text-status-warning-ink" />
  return (
    <TraceToolIcon
      toolName={span.resolvedToolName ?? span.requestedToolName}
      className="shrink-0 text-trace-bar-tool"
    />
  )
}

function spanBarColor(kind: RuntimeTraceSpan['kind']): string {
  if (kind === 'model_call') return 'bg-trace-bar-model'
  if (kind === 'compaction') return 'bg-status-warning-ink'
  return 'bg-trace-bar-tool'
}

export function spanName(span: RuntimeTraceSpan, t: ReturnType<typeof useTranslation>['t']): string {
  if (span.kind === 'model_call') return span.resolvedModelName ?? t('activity.modelCall')
  if (span.kind === 'compaction') return t('activity.compaction')
  return span.resolvedToolName ?? span.requestedToolName ?? t('activity.toolCall')
}

const AUTOMATIC_PERMISSION_SOURCES = new Set([
  'builtin',
  'readonly_proof',
  'mode',
  'mode_fs_command',
  'session_grant',
])

type PermissionActivityKind =
  | 'auto_allowed'
  | 'silently_denied'
  | 'user_approved'
  | 'user_denied'

function PermissionActivityMarker({ span }: { span: RuntimeTraceSpan }) {
  const { t } = useTranslation()
  if (span.kind !== 'tool_call') return null

  const decision = traceStringAttribute(span, 'permissionDecision')
  const source = traceStringAttribute(span, 'permissionDecisionSource')
  const kind = permissionActivityKind(decision, source)
  if (!kind) return null

  const sourceLabel = kind === 'auto_allowed' && source
    ? t(`activity.permissionActivity.sources.${source}`, { defaultValue: source })
    : null
  const categoryLabel = t(`activity.permissionActivity.outcomes.${kind}`)
  const details = [categoryLabel, sourceLabel]
  const readonlyProofKey = traceStringAttribute(span, 'readonlyProofKey')
  if (readonlyProofKey) {
    details.push(t('activity.permissionActivity.readonlyProof', { key: readonlyProofKey }))
  }
  const ruleId = traceStringAttribute(span, 'permissionRuleId')
  if (source === 'session_grant' && ruleId) {
    details.push(t('activity.permissionActivity.sessionGrantRule', { ruleId }))
  }
  const mode = traceStringAttribute(span, 'permissionMode')
  const modeOrigin = traceStringAttribute(span, 'permissionModeOrigin')
  if (mode || modeOrigin) {
    details.push(t('activity.permissionActivity.modeContext', {
      mode: localizeTraceValue(mode, t),
      origin: localizeTraceValue(modeOrigin, t),
    }))
  }

  const styles: Record<PermissionActivityKind, string> = {
    auto_allowed: 'bg-clay-soft text-clay',
    silently_denied: 'bg-status-danger-soft text-status-danger-ink',
    user_approved: 'bg-status-success-soft text-status-success-ink',
    user_denied: 'bg-status-warning-soft text-status-warning-ink',
  }
  const icons = {
    auto_allowed: ShieldCheck,
    silently_denied: ShieldX,
    user_approved: UserCheck,
    user_denied: UserX,
  }
  const Icon = icons[kind]

  return (
    <span
      data-permission-activity={kind}
      data-permission-source={source ?? undefined}
      title={details.filter(Boolean).join(' · ')}
      className={`inline-flex max-w-full shrink-0 items-center gap-1 rounded-full px-1.5 py-0.5 text-[9px] font-semibold leading-none ${styles[kind]}`}
    >
      <Icon size={10} aria-hidden="true" />
      <span className="truncate">{categoryLabel}{sourceLabel ? ` · ${sourceLabel}` : ''}</span>
    </span>
  )
}

function permissionActivityKind(
  decision: string | null,
  source: string | null,
): PermissionActivityKind | null {
  if (decision === 'allow' && source && AUTOMATIC_PERMISSION_SOURCES.has(source)) {
    return 'auto_allowed'
  }
  if (decision === 'deny' && source === 'builtin') return 'silently_denied'
  if (decision === 'allow' && source === 'user') return 'user_approved'
  if (decision === 'deny' && source === 'user') return 'user_denied'
  return null
}

function traceStringAttribute(span: RuntimeTraceSpan, key: string): string | null {
  const value = span.attributes[key]
  return typeof value === 'string' ? value : null
}

function localizeTraceValue(value: string | null, t: ReturnType<typeof useTranslation>['t']): string {
  if (!value) return t('activity.traceValues.unknown')
  return t(`activity.traceValues.${value}`, { defaultValue: value })
}

function spanStatusDot(status: string): string {
  if (status === 'succeeded' || status === 'completed') return 'bg-status-success'
  if (status === 'running') return 'bg-status-warning'
  if (status === 'cancelled' || status === 'interrupted') return 'bg-ink-faint'
  return 'bg-status-danger'
}
