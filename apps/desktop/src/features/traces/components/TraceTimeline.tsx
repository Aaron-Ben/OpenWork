import { Bot, Minimize2, ShieldCheck, ShieldX, UserCheck, UserX, Wrench } from 'lucide-react'
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
      {tree.roots.map((node) => (
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
      ) : span.kind === 'compaction' ? (
        <Minimize2 size={13} className="shrink-0 text-ink-faint" />
      ) : (
        <Wrench size={13} className="shrink-0 text-ink-faint" />
      )}
      <span className="min-w-0 flex-1">
        <span
          className={`block truncate text-xs ${
            selected ? 'font-medium text-ink' : 'text-ink-soft'
          }`}
        >
          {span.kind === 'model_call'
            ? span.resolvedModelName ?? t('activity.modelCall')
            : span.kind === 'compaction'
              ? t('activity.compaction')
              : span.resolvedToolName ?? span.requestedToolName ?? t('activity.toolCall')}
        </span>
        <PermissionActivityMarker span={span} />
      </span>
      <span className="w-12 shrink-0 text-right font-mono text-[11px] tabular-nums text-ink-faint">
        {row ? formatDuration(row.durationMs) : '—'}
      </span>
      {row ? (
        <span className="h-1 w-16 shrink-0 overflow-hidden rounded-full bg-paper-hover">
          <span
            className={`block h-full rounded-full ${
              span.kind === 'model_call'
                ? 'bg-trace-bar-model'
                : span.kind === 'compaction'
                  ? 'bg-status-warning-ink'
                  : 'bg-trace-bar-tool'
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
      className={`mt-0.5 inline-flex max-w-full items-center gap-1 rounded-full px-1.5 py-0.5 text-[9px] font-semibold leading-none ${styles[kind]}`}
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
