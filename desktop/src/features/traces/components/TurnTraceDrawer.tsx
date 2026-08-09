import { useEffect, useMemo, useRef, useState } from 'react'
import type { TFunction } from 'i18next'
import { Bot, Loader2, Maximize2, MessageSquareText, Minimize2, X } from 'lucide-react'
import { AnimatePresence, motion } from 'motion/react'
import { useTranslation } from 'react-i18next'

import { coreCommands } from '@/bridge/commands'
import type {
  RuntimeTraceContentPolicy,
  RuntimeTraceCompleteness,
  RuntimeTracePayloadSlot,
  RuntimeTraceSpan,
  RuntimeTraceSpanPayload,
  RuntimeTraceSummary,
  RuntimeTurnTrace,
} from '@/bridge/compat'
import { formatBeijingDateTime } from '@/lib/dateTime'
import { useTraceContentStore } from '@/features/settings/traceContentStore'
import { resolveErrorMessage } from '@/lib/commandError'
import {
  buildTraceAttributeSections,
  shouldPollTrace,
  type TraceAttributeRow,
  type TraceListItem,
} from '../traceViewModel'
import { formatDuration } from './TraceList'
import { parseTracePayloadMessages, TracePayloadConversation } from './TracePayloadConversation'
import { spanName, TraceTimeline } from './TraceTimeline'
import { TraceToolIcon } from './traceToolIcons'

export type TraceDrawerSource =
  | { kind: 'turn'; turnId: string }
  | { kind: 'trace'; traceId: string }

interface TurnTraceDrawerProps {
  source: TraceDrawerSource
  summary?: TraceListItem | null
  initialProviderCallId?: string
  onOpenMessage?: (sessionId: string, messageId: string) => void
  onClose: () => void
}

export function loadTraceForSource(source: TraceDrawerSource): Promise<RuntimeTurnTrace> {
  return source.kind === 'turn'
    ? coreCommands.getTrace(source.turnId)
    : coreCommands.getTraceById(source.traceId)
}

export function initialTraceSpanId(
  trace: RuntimeTurnTrace,
  sourceKind: TraceDrawerSource['kind'],
  initialProviderCallId?: string,
): string | null {
  const requested = initialProviderCallId
    ? trace.spans.find((span) => span.providerCallId === initialProviderCallId)
    : null
  const summaryChildren = sourceKind === 'trace'
    ? trace.spans.filter((span) => span.kind === 'model_call' && span.parentSpanId != null)
    : []
  const summarySampling = summaryChildren.find((span) => span.status === 'succeeded')
    ?? summaryChildren[0]
  return requested?.id ?? summarySampling?.id ?? trace.spans[0]?.id ?? null
}

export function TurnTraceDrawer({
  source,
  summary,
  initialProviderCallId,
  onOpenMessage,
  onClose,
}: TurnTraceDrawerProps) {
  const { t } = useTranslation()
  const [trace, setTrace] = useState<RuntimeTurnTrace | null>(null)
  const [selectedSpanId, setSelectedSpanId] = useState<string | null>(null)
  const [error, setError] = useState<string | null>(null)
  const drawerRef = useRef<HTMLElement>(null)
  const previousFocus = useRef<HTMLElement | null>(null)
  const requestGeneration = useRef(0)
  const spans = trace?.spans ?? null
  const sourceId = source.kind === 'turn' ? source.turnId : source.traceId
  const sourceKind = source.kind

  useEffect(() => {
    previousFocus.current = document.activeElement as HTMLElement | null
    drawerRef.current?.focus()
    return () => previousFocus.current?.focus()
  }, [])

  useEffect(() => {
    const generation = ++requestGeneration.current
    let active = true
    setTrace(null)
    setError(null)
    void loadTraceForSource(source)
      .then((value) => {
        if (!active || generation !== requestGeneration.current) return
        setTrace(value)
        setSelectedSpanId(initialTraceSpanId(value, sourceKind, initialProviderCallId))
      })
      .catch((reason) => {
        if (active && generation === requestGeneration.current) {
          setError(resolveErrorMessage(reason))
        }
      })
    return () => {
      active = false
      requestGeneration.current += 1
    }
  }, [initialProviderCallId, sourceId, sourceKind])

  const shouldPoll = shouldPollTrace(summary?.status ?? trace?.summary.status, spans ?? [])
  useEffect(() => {
    if (!shouldPoll) return
    const generation = requestGeneration.current
    const timer = window.setInterval(() => {
      void loadTraceForSource(source)
        .then((value) => {
          if (generation !== requestGeneration.current) return
          setTrace(value)
          setError(null)
          setSelectedSpanId((current) =>
            current && value.spans.some((span) => span.id === current)
              ? current
              : value.spans[0]?.id ?? null,
          )
        })
        .catch((reason) => {
          if (generation === requestGeneration.current) {
            setError(resolveErrorMessage(reason))
          }
        })
    }, 2_000)
    return () => window.clearInterval(timer)
  }, [shouldPoll, sourceId, sourceKind])

  useEffect(() => {
    if (!selectedSpanId) return
    drawerRef.current
      ?.querySelector<HTMLElement>(`[data-span-id="${CSS.escape(selectedSpanId)}"]`)
      ?.scrollIntoView({ block: 'nearest' })
  }, [selectedSpanId])

  const selected = useMemo(
    () => spans?.find((span) => span.id === selectedSpanId) ?? null,
    [selectedSpanId, spans],
  )
  const tokenTotal = useMemo(
    () => (spans ?? []).reduce(
      (sum, span) => sum + (span.totalTokens ?? 0),
      0,
    ),
    [spans],
  )

  return (
    <AnimatePresence>
      <motion.div
        className="fixed inset-0 z-40 bg-ink/10 backdrop-blur-[1px]"
        initial={{ opacity: 0 }}
        animate={{ opacity: 1 }}
        exit={{ opacity: 0 }}
        onMouseDown={(event) => { if (event.target === event.currentTarget) onClose() }}
      >
        <motion.aside
          ref={drawerRef}
          role="dialog"
          aria-modal="true"
          aria-label={t('activity.traceDetail')}
          tabIndex={-1}
          initial={{ opacity: 0, x: 32 }}
          animate={{ opacity: 1, x: 0 }}
          exit={{ opacity: 0, x: 32 }}
          className="absolute inset-y-0 right-0 flex w-[min(1180px,98vw)] flex-col border-l border-line bg-paper shadow-[-18px_0_45px_rgba(20,20,19,0.12)] outline-none max-[640px]:w-full"
          onKeyDown={(event) => { if (event.key === 'Escape') onClose() }}
        >
          <TraceSummaryHeader
            title={summary?.title ?? t('activity.traceDetail')}
            sourceId={sourceId}
            summary={trace?.summary ?? summary ?? null}
            completeness={trace?.completeness ?? null}
            tokenTotal={trace ? tokenTotal : undefined}
            onClose={onClose}
          />

          <div className="grid min-h-0 flex-1 grid-cols-[minmax(0,1.4fr)_minmax(340px,0.7fr)] max-[760px]:grid-cols-1 max-[760px]:overflow-auto">
            <section className="min-h-0 overflow-auto border-r border-line bg-surface/55 p-4 max-[760px]:border-b max-[760px]:border-r-0" aria-label={t('activity.timeline')}>
              {!spans && !error ? <TraceLoading /> : null}
              {error ? <p className="rounded-xl bg-status-danger-soft p-3 text-sm text-status-danger-ink">{error}</p> : null}
              {spans?.length === 0 ? <p className="text-sm text-ink-faint">{t('activity.noSpans')}</p> : null}
              {spans && spans.length > 0 ? (
                <TraceTimeline spans={spans} selectedSpanId={selectedSpanId} onSelect={(span) => setSelectedSpanId(span.id)} />
              ) : null}
            </section>
            <section className="min-h-0 overflow-auto p-5" aria-label={t('activity.spanDetail')}>
              {selected ? (
                <SpanDetail span={selected} onOpenMessage={onOpenMessage} />
              ) : (
                <p className="text-sm text-ink-faint">{t('activity.selectSpan')}</p>
              )}
            </section>
          </div>
        </motion.aside>
      </motion.div>
    </AnimatePresence>
  )
}

function TraceLoading() {
  const { t } = useTranslation()
  return <div className="grid gap-2" aria-label={t('activity.loadingTrace')}>{[0, 1, 2].map((item) => <div key={item} className="h-16 animate-pulse rounded-xl bg-surface" />)}</div>
}

export function TraceSummaryHeader({
  title,
  sourceId,
  summary,
  completeness,
  tokenTotal,
  onClose,
}: {
  title: string
  sourceId: string
  summary: RuntimeTraceSummary | null
  completeness: RuntimeTraceCompleteness | null
  tokenTotal?: number
  onClose: () => void
}) {
  const { t } = useTranslation()
  const durationMs = summary
    ? Math.max(0, Date.parse(summary.endedAt ?? new Date().toISOString()) - Date.parse(summary.startedAt))
    : null
  const captured = completeness
    ? completeness.capturedModelCalls + completeness.capturedToolCalls
    : null
  const expected = completeness
    ? completeness.expectedModelCalls + completeness.expectedToolCalls
    : null
  const incomplete = completeness != null && completeness.state !== 'complete'
  const missing = captured == null || expected == null ? 0 : Math.max(0, expected - captured)
  const completenessIssues = completeness
    ? [
        missing > 0 ? t('activity.traceMissing', { count: missing }) : null,
        completeness.orphanToolSpans > 0
          ? t('activity.traceOrphanSpans', { count: completeness.orphanToolSpans })
          : null,
        completeness.runningSpans > 0
          ? t('activity.traceRunningSpans', { count: completeness.runningSpans })
          : null,
        completeness.outcomeUnknownSpans > 0
          ? t('activity.traceUnknownSpans', { count: completeness.outcomeUnknownSpans })
          : null,
      ].filter((value): value is string => value != null)
    : []
  const completenessNote = completeness && incomplete
    ? [t(`activity.completenessState.${completeness.state}`), ...completenessIssues].join(' · ')
    : null
  const overview = [
    [t('activity.totalDuration'), formatDuration(durationMs)],
    [t('activity.modelCallsLabel'), summary?.modelSubmissionCount ?? '—'],
    [t('activity.toolCallsLabel'), summary?.toolCallCount ?? '—'],
    [t('activity.tokensLabel'), (tokenTotal ?? summary?.totalTokens)?.toLocaleString() ?? '—'],
    [t('activity.completenessLabel'), captured == null || expected == null ? '—' : `${captured} / ${expected}`],
  ] as const

  return (
    <header className="border-b border-line px-5 py-4">
      <div className="flex items-start gap-4">
        <div className="min-w-0 flex-1">
          <div className="flex min-w-0 items-center gap-2">
            <h2 className="truncate text-lg font-semibold text-ink">{title}</h2>
            {summary ? <SpanStatusChip status={summary.status} /> : null}
          </div>
          <p className="mt-1 truncate font-mono text-[10px] text-ink-faint">{sourceId}</p>
        </div>
        <button
          type="button"
          onClick={onClose}
          className="grid size-9 shrink-0 place-items-center rounded-full bg-surface text-ink-soft transition-colors hover:bg-paper-hover hover:text-ink"
          aria-label={t('activity.close')}
        >
          <X size={16} />
        </button>
      </div>
      <div
        data-trace-overview="true"
        className="mt-4 grid grid-cols-5 overflow-hidden rounded-2xl bg-surface max-[640px]:grid-cols-2"
      >
        {overview.map(([label, value], index) => (
          <div
            key={label}
            data-trace-completeness={index === 4 && completeness ? completeness.state : undefined}
            role={index === 4 && incomplete ? 'alert' : undefined}
            title={index === 4 ? completenessNote ?? undefined : undefined}
            className={`min-w-0 px-5 py-3 ${index > 0 ? 'border-l border-line/60 max-[640px]:border-l-0' : ''} ${
              index === 4 && completeness?.state === 'none'
                ? 'bg-status-danger-soft text-status-danger-ink'
                : index === 4 && incomplete
                  ? 'bg-status-warning-soft text-status-warning-ink'
                  : ''
            } ${index === 4 ? 'max-[640px]:col-span-2' : ''}`}
          >
            <div className="truncate text-[10px] font-semibold uppercase tracking-wide text-ink-faint">{label}</div>
            <div className="mt-1 truncate font-mono text-sm font-semibold tabular-nums text-ink">{value}</div>
            {index === 4 && completenessNote ? (
              <div data-trace-completeness-note="true" className="mt-0.5 whitespace-normal break-words text-[9px] font-medium leading-tight">{completenessNote}</div>
            ) : null}
          </div>
        ))}
      </div>
    </header>
  )
}

export function SpanDetail({
  span,
  onOpenMessage,
}: {
  span: RuntimeTraceSpan
  onOpenMessage?: (sessionId: string, messageId: string) => void
}) {
  const { t } = useTranslation()
  const duration = span.endedAt
    ? Math.max(0, Date.parse(span.endedAt) - Date.parse(span.startedAt))
    : Math.max(0, Date.now() - Date.parse(span.startedAt))
  const detailFields: Array<[string, string | number]> = [
    [t('activity.startedAt'), formatBeijingDateTime(span.startedAt)],
    [t('activity.endedAt'), span.endedAt ? formatBeijingDateTime(span.endedAt) : '—'],
    [t('activity.permissionWait'), span.permissionWaitMs == null ? '—' : formatDuration(span.permissionWaitMs)],
    [t('activity.providerRequestId'), span.providerRequestId ?? '—'],
  ]
  detailFields.splice(2, 0, [t('activity.attempts'), span.attemptCount ?? '—'])
  const attributeSections = buildTraceAttributeSections(span)
  const properties = [
    ...attributeSections.p0.map((row) => ({
      key: row.key,
      label: t(`activity.traceFields.${row.key}`, { defaultValue: row.key }),
      value: localizeTraceAttributeValue(row, t),
    })),
    ...detailFields.map(([label, value], index) => ({ key: `detail-${index}`, label, value: String(value) })),
    ...attributeSections.p1.map((row) => ({
      key: row.key,
      label: t(`activity.traceFields.${row.key}`, { defaultValue: row.key }),
      value: localizeTraceAttributeValue(row, t),
    })),
  ].sort((left, right) => tracePropertyRank(left.key) - tracePropertyRank(right.key))
  const callIndex = typeof span.attributes.modelCallIndex === 'number'
    ? span.attributes.modelCallIndex
    : null
  return (
    <div>
      <div className="flex items-center gap-3">
        <span className="grid size-9 shrink-0 place-items-center rounded-lg bg-paper-hover text-ink-soft">
          {span.kind === 'model_call'
            ? <Bot size={16} />
            : span.kind === 'compaction'
              ? <Minimize2 size={16} />
              : <TraceToolIcon toolName={span.resolvedToolName ?? span.requestedToolName} size={16} />}
        </span>
        <div className="min-w-0 flex-1">
          <h3 className="truncate text-sm font-semibold text-ink">
            {span.kind === 'model_call'
              ? span.resolvedModelName ?? t('activity.modelCall')
              : span.kind === 'compaction'
                ? t('activity.compaction')
                : span.resolvedToolName ?? span.requestedToolName ?? t('activity.toolCall')}
          </h3>
          <div className="mt-1 flex flex-wrap items-center gap-1.5 text-[11px] text-ink-faint">
            <SpanStatusChip status={span.status} />
            <span aria-hidden="true">·</span>
            <span className="font-mono tabular-nums">{formatDuration(duration)}</span>
            {callIndex == null ? null : <><span aria-hidden="true">·</span><span>{t('activity.callOrdinal', { count: callIndex })}</span></>}
          </div>
        </div>
      </div>

      <TokenComposition span={span} />
      <TracePayloadSection span={span} onOpenMessage={onOpenMessage} />
      <TraceProperties rows={properties} />
      {span.errorMessage ? (
        <div className="mt-4 rounded-xl bg-status-danger-soft p-3 text-xs text-status-danger-ink">
          <div className="font-semibold">{span.errorCode ?? t('activity.error')}</div>
          <p className="mt-1 whitespace-pre-wrap">{span.errorMessage}</p>
        </div>
      ) : null}
    </div>
  )
}

function TokenComposition({ span }: { span: RuntimeTraceSpan }) {
  const { t } = useTranslation()
  const rows = [
    { key: 'input', label: t('activity.inputTokens'), value: span.inputTokens, color: 'bg-clay' },
    { key: 'output', label: t('activity.outputTokens'), value: span.outputTokens, color: 'bg-trace-bar-tool' },
    { key: 'reasoning', label: t('activity.reasoningTokens'), value: span.reasoningTokens, color: 'bg-ink-faint' },
    { key: 'cached', label: t('activity.cachedTokens'), value: span.cachedInputTokens, color: 'bg-line-strong' },
  ].filter((row): row is typeof row & { value: number } => row.value != null)
  if (rows.length === 0) return null

  // cache 是 input 的子集，reasoning 是 output 的子集；条形图只画互斥部分，避免合计被重复放大。
  const exclusiveValues = new Map<string, number>([
    ['input', Math.max(0, (span.inputTokens ?? 0) - (span.cachedInputTokens ?? 0))],
    ['output', Math.max(0, (span.outputTokens ?? 0) - (span.reasoningTokens ?? 0))],
    ['reasoning', span.reasoningTokens ?? 0],
    ['cached', span.cachedInputTokens ?? 0],
  ])
  const segments = rows
    .map((row) => ({ ...row, exclusiveValue: exclusiveValues.get(row.key) ?? 0 }))
    .filter((row) => row.exclusiveValue > 0)
  const barTotal = Math.max(1, segments.reduce((sum, row) => sum + row.exclusiveValue, 0))
  const total = span.totalTokens ?? (span.inputTokens ?? 0) + (span.outputTokens ?? 0)

  return (
    <section data-token-composition="true" className="mt-5">
      <div className="flex items-center justify-between gap-3">
        <h4 className="text-xs font-semibold text-ink-soft">{t('activity.tokenComposition')}</h4>
        <span className="font-mono text-xs font-semibold tabular-nums text-ink">{total.toLocaleString()}</span>
      </div>
      <div className="mt-3 flex h-2 overflow-hidden rounded-full bg-surface" aria-hidden="true">
        {segments.map((row) => (
          <span
            key={row.key}
            data-token-segment={row.key}
            className={row.color}
            style={{ width: `${(row.exclusiveValue / barTotal) * 100}%` }}
          />
        ))}
      </div>
      <dl className="mt-3 divide-y divide-line/70">
        {rows.map((row) => (
          <div key={row.key} className="flex items-center gap-2 py-2 text-xs">
            <span className={`size-2 rounded-sm ${row.color}`} aria-hidden="true" />
            <dt className="flex-1 text-ink-soft">{row.label}</dt>
            <dd className="font-mono tabular-nums text-ink">{row.value.toLocaleString()}</dd>
          </div>
        ))}
      </dl>
    </section>
  )
}

interface TraceProperty {
  key: string
  label: string
  value: string
}

const TRACE_PROPERTY_PRIORITY = [
  'finishReason', 'permissionDecision', 'trigger',
  'detail-2', 'detail-0', 'detail-1',
  'ttftMs', 'streamMs', 'requestBuildMs', 'executionMs',
  'toolChoice', 'requestMessageCount', 'toolDefinitionCount',
  'prepareMs', 'summaryMs', 'persistenceMs', 'installMs',
] as const

function tracePropertyRank(key: string): number {
  const index = TRACE_PROPERTY_PRIORITY.indexOf(key as typeof TRACE_PROPERTY_PRIORITY[number])
  return index === -1 ? TRACE_PROPERTY_PRIORITY.length : index
}

function TraceProperties({ rows }: { rows: TraceProperty[] }) {
  const { t } = useTranslation()
  const [expanded, setExpanded] = useState(false)
  return (
    <section data-trace-properties="true" className="mt-5">
      <div className="flex items-center justify-between gap-3">
        <h4 className="text-xs font-semibold text-ink-soft">{t('activity.traceAttributes')}</h4>
        {rows.length > 8 ? (
          <button type="button" onClick={() => setExpanded((value) => !value)} className="text-[11px] font-medium text-clay hover:underline">
            {expanded ? t('activity.collapse') : t('activity.expandAllProperties', { count: rows.length })}
          </button>
        ) : null}
      </div>
      <dl className="mt-3 grid grid-cols-2 gap-2 max-[460px]:grid-cols-1">
        {rows.map((row, index) => (
          <div key={row.key} hidden={!expanded && index >= 8} className="min-w-0 rounded-xl bg-surface px-3 py-2.5">
            <dt className="truncate text-[10px] text-ink-faint" title={row.label}>{row.label}</dt>
            <dd data-trace-property-value="true" className="mt-1 whitespace-pre-wrap break-all font-mono text-xs text-ink">{row.value}</dd>
          </div>
        ))}
      </dl>
    </section>
  )
}

const TRACE_PAYLOAD_SLOTS: RuntimeTracePayloadSlot[] = [
  'request',
  'system_context',
  'tool_definitions',
  'response',
]

export const TRACE_PAYLOAD_RENDER_LIMIT_CHARS = 64 * 1024
export const TRACE_PAYLOAD_RENDER_LIMIT_LINES = 200

interface TracePayloadPreview {
  full: string
  preview: string
  limited: boolean
}

export function buildTracePayloadPreview(body: unknown): TracePayloadPreview {
  let full: string
  try {
    full = JSON.stringify(body, null, 2) ?? String(body)
  } catch {
    full = String(body)
  }

  let end = Math.min(full.length, TRACE_PAYLOAD_RENDER_LIMIT_CHARS)
  let cursor = 0
  for (let line = 0; line < TRACE_PAYLOAD_RENDER_LIMIT_LINES; line += 1) {
    const next = full.indexOf('\n', cursor)
    if (next === -1) {
      cursor = full.length
      break
    }
    cursor = next + 1
  }
  if (cursor > 0) end = Math.min(end, cursor)
  return {
    full,
    preview: full.slice(0, end),
    limited: end < full.length,
  }
}

export async function loadTracePayloadWhenExpanded(
  expanded: boolean,
  spanId: string,
  slot: RuntimeTracePayloadSlot,
  fetchPayload: typeof coreCommands.getSpanPayload = coreCommands.getSpanPayload,
): Promise<RuntimeTraceSpanPayload | null | undefined> {
  if (!expanded) return undefined
  return fetchPayload(spanId, slot)
}

function TracePayloadSection({
  span,
  onOpenMessage,
}: {
  span: RuntimeTraceSpan
  onOpenMessage?: (sessionId: string, messageId: string) => void
}) {
  const { t } = useTranslation()
  return (
    <section className="mt-5" aria-label={t('activity.payloads.title')}>
      <h4 className="text-xs font-semibold text-ink-soft">{t('activity.payloads.title')}</h4>
      <div data-trace-payload-grid="true" className="mt-3 grid grid-cols-2 gap-2">
        {TRACE_PAYLOAD_SLOTS.map((slot) => (
          <TracePayloadSlotDisclosure
            key={`${span.id}:${slot}`}
            span={span}
            slot={slot}
            onOpenMessage={onOpenMessage}
          />
        ))}
      </div>
    </section>
  )
}

type PayloadState =
  | { status: 'idle' }
  | { status: 'loading' }
  | { status: 'loaded'; payload: RuntimeTraceSpanPayload | null }
  | { status: 'error'; message: string }

function TracePayloadSlotDisclosure({
  span,
  slot,
  onOpenMessage,
}: {
  span: RuntimeTraceSpan
  slot: RuntimeTracePayloadSlot
  onOpenMessage?: (sessionId: string, messageId: string) => void
}) {
  const { t } = useTranslation()
  const [open, setOpen] = useState(false)
  const [payloadState, setPayloadState] = useState<PayloadState>({ status: 'idle' })
  const mounted = useRef(true)
  const rowRef = useRef<HTMLButtonElement>(null)
  const responseMessageId = slot === 'response' ? span.responseMessageId : null
  const storedElsewhere = payloadStoredElsewhere(span, slot)

  useEffect(() => {
    mounted.current = true
    return () => {
      mounted.current = false
    }
  }, [])

  // 成功响应的正文在会话里：整行就是跳转入口，不开窗也不拉取。
  if (responseMessageId) {
    return (
      <button
        type="button"
        data-payload-slot={slot}
        className="flex min-h-12 w-full items-center gap-2 rounded-xl border border-clay/35 bg-surface px-3 py-2 text-left text-xs font-medium text-clay transition-colors hover:bg-clay-soft focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-clay/35"
        onClick={() => onOpenMessage?.(span.sessionId, responseMessageId)}
      >
        <MessageSquareText size={13} className="shrink-0" />
        {t('activity.payloads.openMessage')}
      </button>
    )
  }

  // 正文不在 Trace 里的槽位（工具调用、压缩）：陈述事实，不可交互。
  if (storedElsewhere) {
    return (
      <div data-payload-slot={slot} className="min-h-12 rounded-xl border border-line bg-surface px-3 py-2">
        <span className="text-xs font-medium text-ink">{t(`activity.payloads.slots.${slot}`)}</span>
        <p className="mt-0.5 text-[11px] leading-5 text-ink-faint">{t(storedElsewhere)}</p>
      </div>
    )
  }

  async function handleOpen() {
    setOpen(true)
    if (payloadState.status !== 'idle') return
    setPayloadState({ status: 'loading' })
    try {
      const payload = await loadTracePayloadWhenExpanded(true, span.id, slot)
      if (mounted.current) setPayloadState({ status: 'loaded', payload: payload ?? null })
    } catch (reason) {
      if (mounted.current) {
        setPayloadState({ status: 'error', message: resolveErrorMessage(reason) })
      }
    }
  }

  const handleClose = () => {
    setOpen(false)
    rowRef.current?.focus()
  }

  return (
    <>
      <button
        ref={rowRef}
        type="button"
        data-payload-slot={slot}
        onClick={() => void handleOpen()}
        className="flex min-h-12 w-full items-center gap-2 rounded-xl border border-line bg-surface px-3 py-2 text-left text-xs font-medium text-ink transition-colors hover:border-clay/40 hover:bg-clay-soft focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-clay/35"
      >
        <Maximize2 size={12} className="shrink-0 text-ink-faint" />
        <span>{t(`activity.payloads.slots.${slot}`)}</span>
        {payloadState.status === 'loading' ? (
          <Loader2
            size={12}
            className="ml-auto shrink-0 animate-spin text-ink-faint"
            aria-label={t('activity.payloads.loading')}
          />
        ) : null}
      </button>
      <AnimatePresence>
        {open ? (
          <TracePayloadModal
            slot={slot}
            spanTitle={spanName(span, t)}
            state={payloadState}
            onClose={handleClose}
          />
        ) : null}
      </AnimatePresence>
    </>
  )
}

/** 正文查看窗：槽位内容在居中大窗里展示，详情列保持紧凑。 */
export function TracePayloadModal({
  slot,
  spanTitle,
  state,
  onClose,
}: {
  slot: RuntimeTracePayloadSlot
  spanTitle: string
  state: PayloadState
  onClose: () => void
}) {
  const { t } = useTranslation()
  const policy = useTraceContentStore((store) => store.policy)
  const panelRef = useRef<HTMLDivElement>(null)

  useEffect(() => {
    panelRef.current?.focus()
  }, [])

  return (
    <motion.div
      className="fixed inset-0 z-50 grid place-items-center bg-ink/20 p-4 backdrop-blur-[1px]"
      initial={{ opacity: 0 }}
      animate={{ opacity: 1 }}
      exit={{ opacity: 0 }}
      onMouseDown={(event) => { if (event.target === event.currentTarget) onClose() }}
    >
      <motion.div
        ref={panelRef}
        role="dialog"
        aria-modal="true"
        aria-label={`${t(`activity.payloads.slots.${slot}`)} · ${spanTitle}`}
        tabIndex={-1}
        data-payload-modal={slot}
        initial={{ opacity: 0, scale: 0.97, y: 8 }}
        animate={{ opacity: 1, scale: 1, y: 0 }}
        exit={{ opacity: 0, scale: 0.97, y: 8 }}
        transition={{ duration: 0.16, ease: 'easeOut' }}
        className="flex max-h-[85vh] w-[min(960px,92vw)] flex-col overflow-hidden rounded-2xl border border-line bg-paper shadow-[0_24px_70px_rgba(20,20,19,0.25)] outline-none"
        onKeyDown={(event) => {
          if (event.key === 'Escape') {
            event.stopPropagation()
            onClose()
          }
        }}
      >
        <header className="flex items-center gap-3 border-b border-line px-4 py-3">
          <div className="min-w-0 flex-1">
            <h3 className="truncate text-sm font-semibold text-ink">
              {t(`activity.payloads.slots.${slot}`)}
            </h3>
            <p className="mt-0.5 truncate text-[11px] text-ink-faint">{spanTitle}</p>
          </div>
          <button
            type="button"
            onClick={onClose}
            aria-label={t('activity.payloads.close')}
            className="rounded-lg p-1.5 text-ink-faint transition-colors hover:bg-paper-hover hover:text-ink focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-clay/35"
          >
            <X size={15} />
          </button>
        </header>
        <div className="min-h-0 flex-1 overflow-auto p-4">
          {state.status === 'error' ? (
            <p role="alert" className="text-xs text-status-danger-ink">{state.message}</p>
          ) : state.status === 'loaded' ? (
            state.payload ? (
              <TracePayloadBody payload={state.payload} relaxed />
            ) : (
              <MissingTracePayload policy={policy} />
            )
          ) : (
            <p className="text-xs text-ink-faint">{t('activity.payloads.loading')}</p>
          )}
        </div>
      </motion.div>
    </motion.div>
  )
}

function payloadStoredElsewhere(
  span: RuntimeTraceSpan,
  slot: RuntimeTracePayloadSlot,
): 'activity.payloads.toolStoredInChat' | 'activity.payloads.compactionHasNoPayload' | null {
  if (span.kind === 'compaction') return 'activity.payloads.compactionHasNoPayload'
  if (span.kind !== 'tool_call') return null
  const resultPersisted = span.attributes.resultPersisted
  if (
    slot !== 'response'
    || resultPersisted === true
    || (resultPersisted == null && span.status === 'succeeded')
  ) {
    return 'activity.payloads.toolStoredInChat'
  }
  return null
}

export function MissingTracePayload({ policy }: { policy: RuntimeTraceContentPolicy }) {
  const { t } = useTranslation()
  return (
    <div className="text-xs leading-5 text-ink-faint">
      <p>{t('activity.payloads.missing')}</p>
      {policy === 'full' ? null : <p>{t('activity.payloads.currentPolicyLimited')}</p>}
    </div>
  )
}

export function TracePayloadBody({
  payload,
  relaxed = false,
}: {
  payload: RuntimeTraceSpanPayload
  /** relaxed（模态窗）时取消限高，由外层容器滚动；内联（详情列）时限高。 */
  relaxed?: boolean
}) {
  const { t } = useTranslation()
  const [showAll, setShowAll] = useState(false)
  const rendered = useMemo(() => buildTracePayloadPreview(payload.body), [payload.body])
  // request 槽位存的是组装后的 Message[]：优先渲染成对话形态（长块自动收起），
  // 形状不符或用户主动切换时退回原始 JSON。
  const messages = useMemo(
    () => (payload.slot === 'request' ? parseTracePayloadMessages(payload.body) : null),
    [payload.body, payload.slot],
  )
  const [view, setView] = useState<'conversation' | 'json'>('conversation')
  const showConversation = messages !== null && view === 'conversation'
  const text = showAll ? rendered.full : rendered.preview
  return (
    <div>
      <div className="mb-2 flex flex-wrap items-center gap-2 text-[11px] text-ink-faint">
        <span>{t('activity.payloads.storedSize', { size: formatByteSize(payload.byteSize) })}</span>
        {payload.truncated ? (
          <span className="rounded-full bg-status-warning-soft px-2 py-0.5 text-status-warning-ink">
            {t('activity.payloads.truncated', {
              size: formatByteSize(payload.originalByteSize ?? payload.byteSize),
            })}
          </span>
        ) : null}
        {messages ? (
          <span
            className="ml-auto inline-flex overflow-hidden rounded-md border border-line"
            data-payload-view-toggle="true"
          >
            <PayloadViewButton active={showConversation} onClick={() => setView('conversation')}>
              {t('activity.payloads.viewMessages')}
            </PayloadViewButton>
            <PayloadViewButton active={!showConversation} onClick={() => setView('json')}>
              JSON
            </PayloadViewButton>
          </span>
        ) : null}
      </div>
      {showConversation ? (
        <TracePayloadConversation messages={messages} relaxed={relaxed} />
      ) : (
        <>
          <pre className={`overflow-auto whitespace-pre-wrap break-all rounded-lg bg-surface p-3 font-mono text-[11px] leading-5 text-ink-soft ${relaxed ? '' : 'max-h-[32rem]'}`}>
            {text}{rendered.limited && !showAll ? '\n…' : ''}
          </pre>
          {rendered.limited ? (
            <button
              type="button"
              className="mt-2 text-xs font-medium text-clay hover:underline"
              onClick={() => setShowAll((value) => !value)}
            >
              {showAll ? t('activity.payloads.showPreview') : t('activity.payloads.showAll')}
            </button>
          ) : null}
        </>
      )}
    </div>
  )
}

function PayloadViewButton({
  active,
  onClick,
  children,
}: {
  active: boolean
  onClick: () => void
  children: React.ReactNode
}) {
  return (
    <button
      type="button"
      aria-pressed={active}
      onClick={onClick}
      className={`px-2 py-0.5 text-[10px] font-medium transition-colors ${
        active ? 'bg-clay-soft text-clay' : 'text-ink-faint hover:text-ink-soft'
      }`}
    >
      {children}
    </button>
  )
}

export function formatByteSize(bytes: number): string {
  if (bytes < 1_024) return `${bytes} B`
  if (bytes < 1_024 * 1_024) return `${(bytes / 1_024).toFixed(1)} KiB`
  return `${(bytes / (1_024 * 1_024)).toFixed(1)} MiB`
}

function SpanStatusChip({ status }: { status: string }) {
  const { t } = useTranslation()
  const classes = status === 'succeeded' || status === 'completed'
    ? 'bg-status-success-soft text-status-success'
    : status === 'running'
      ? 'bg-status-warning-soft text-status-warning-ink'
      : status === 'cancelled' || status === 'interrupted'
        ? 'bg-paper-hover text-ink-soft'
        : 'bg-status-danger-soft text-status-danger-ink'
  return (
    <span className={`inline-flex items-center rounded-full px-2 py-0.5 text-[10px] font-semibold ${classes}`}>
      {t(`activity.status.${status}`, { defaultValue: localizeTraceValue(status, t) })}
    </span>
  )
}

const LOCALIZED_TRACE_ATTRIBUTE_VALUES = new Set([
  'finishReason', 'errorPhase', 'deliveryState', 'permissionPolicy',
  'permissionDecision', 'permissionDecisionSource', 'permissionRuleScope', 'thinkingMode', 'toolChoice',
])

function localizeTraceAttributeValue(row: TraceAttributeRow, t: TFunction): string {
  if (LOCALIZED_TRACE_ATTRIBUTE_VALUES.has(row.key) || row.value === 'true' || row.value === 'false') {
    return localizeTraceValue(row.value, t)
  }
  return row.value
}

function localizeTraceValue(value: string, t: TFunction): string {
  return t(`activity.traceValues.${value}`, { defaultValue: value })
}
