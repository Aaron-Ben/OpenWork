import { useEffect, useMemo, useRef, useState } from 'react'
import type { TFunction } from 'i18next'
import { Bot, CircleAlert, Clock3, Loader2, Maximize2, MessageSquareText, Minimize2, Wrench, X } from 'lucide-react'
import { AnimatePresence, motion } from 'motion/react'
import { useTranslation } from 'react-i18next'

import { coreCommands } from '@/bridge/commands'
import type {
  RuntimeTraceContentPolicy,
  RuntimeTraceCompleteness,
  RuntimeTracePayloadSlot,
  RuntimeTraceSpan,
  RuntimeTraceSpanPayload,
  RuntimeTurnTrace,
} from '@/bridge/compat'
import { formatBeijingDateTime } from '@/lib/dateTime'
import { useTraceContentStore } from '@/stores/traceContentStore'
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
          className="absolute inset-y-0 right-0 flex w-[min(1120px,96vw)] flex-col border-l border-line bg-paper shadow-[-18px_0_45px_rgba(20,20,19,0.12)] outline-none max-[640px]:w-full"
          onKeyDown={(event) => { if (event.key === 'Escape') onClose() }}
        >
          <header className="border-b border-line px-5 py-4">
            <div className="flex items-start gap-4">
              <div className="min-w-0 flex-1">
                <h2 className="truncate text-base font-semibold text-ink">
                  {summary?.title ?? t('activity.traceDetail')}
                </h2>
                <p className="mt-1 truncate font-mono text-[11px] text-ink-faint">{sourceId}</p>
              </div>
              <button type="button" onClick={onClose} className="rounded-lg p-2 hover:bg-paper-hover" aria-label={t('activity.close')}>
                <X size={18} />
              </button>
            </div>
            <div className="mt-4 flex flex-wrap gap-2 text-xs text-ink-soft">
              {summary ? <SummaryPill icon={<Clock3 size={12} />} label={formatDuration(summary.durationMs)} /> : null}
              <SummaryPill icon={<Bot size={12} />} label={t('activity.modelCalls', { count: spans?.filter((span) => span.kind === 'model_call').length ?? summary?.modelCallCount ?? 0 })} />
              <SummaryPill icon={<Wrench size={12} />} label={t('activity.toolCalls', { count: spans?.filter((span) => span.kind === 'tool_call').length ?? summary?.toolCallCount ?? 0 })} />
              <SummaryPill label={t('activity.tokens', { count: tokenTotal })} />
              {trace ? <TraceCompletenessSummary completeness={trace.completeness} /> : null}
            </div>
          </header>

          <div className="grid min-h-0 flex-1 grid-cols-[minmax(0,1.25fr)_minmax(320px,0.75fr)] max-[760px]:grid-cols-1 max-[760px]:overflow-auto">
            <section className="min-h-0 overflow-auto border-r border-line p-4 max-[760px]:border-b max-[760px]:border-r-0" aria-label={t('activity.timeline')}>
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

function SummaryPill({ icon, label }: { icon?: React.ReactNode; label: string }) {
  return <span className="inline-flex items-center gap-1 rounded-full bg-surface px-2.5 py-1">{icon}{label}</span>
}

export function TraceCompletenessSummary({
  completeness,
}: {
  completeness: RuntimeTraceCompleteness
}) {
  const { t } = useTranslation()
  const captured = completeness.capturedModelCalls + completeness.capturedToolCalls
  const expected = completeness.expectedModelCalls + completeness.expectedToolCalls
  const missing = Math.max(0, expected - captured)
  const incomplete = completeness.state !== 'complete'
  const classes = completeness.state === 'complete'
    ? 'bg-surface text-ink-soft'
    : completeness.state === 'partial'
      ? 'border border-status-warning-border bg-status-warning-soft font-semibold text-status-warning-ink'
      : 'border border-status-danger-border bg-status-danger-soft font-semibold text-status-danger-ink'

  return (
    <span
      data-trace-completeness={completeness.state}
      role={incomplete ? 'alert' : undefined}
      className={`inline-flex items-center gap-1 rounded-full px-2.5 py-1 ${classes}`}
    >
      {incomplete ? <CircleAlert size={12} aria-hidden="true" /> : null}
      {t('activity.traceCompleteness', {
        state: t(`activity.completeness.${completeness.state}`),
        captured,
        expected,
      })}
      {missing > 0 ? ` · ${t('activity.traceMissing', { count: missing })}` : null}
    </span>
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
  const tokenStats: Array<[string, number | null]> = [
    [t('activity.inputTokens'), span.inputTokens],
    [t('activity.outputTokens'), span.outputTokens],
    [t('activity.cachedTokens'), span.cachedInputTokens],
    [t('activity.reasoningTokens'), span.reasoningTokens],
    [t('activity.totalTokens'), span.totalTokens],
  ]
  const visibleTokenStats = tokenStats.filter(([, value]) => value != null)
  const detailFields: Array<[string, string | number]> = [
    [t('activity.startedAt'), formatBeijingDateTime(span.startedAt)],
    [t('activity.endedAt'), span.endedAt ? formatBeijingDateTime(span.endedAt) : '—'],
    [t('activity.permissionWait'), span.permissionWaitMs == null ? '—' : formatDuration(span.permissionWaitMs)],
    [t('activity.providerRequestId'), span.providerRequestId ?? '—'],
  ]
  if (span.kind !== 'compaction') {
    detailFields.splice(2, 0, [t('activity.attempts'), span.attemptCount ?? '—'])
  }
  const attributeSections = buildTraceAttributeSections(span)
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
          <div className="mt-1 flex items-center gap-2">
            <SpanStatusChip status={span.status} />
            <span className="font-mono text-[11px] tabular-nums text-ink-faint">
              {formatDuration(duration)}
            </span>
          </div>
        </div>
      </div>

      {visibleTokenStats.length > 0 ? (
        <div className="mt-4 grid grid-cols-2 gap-1.5 min-[480px]:grid-cols-3">
          {visibleTokenStats.map(([label, value]) => (
            <div key={label} className="rounded-lg border border-line px-2.5 py-1.5">
              <div className="text-[10px] text-ink-faint">{label}</div>
              <div className="mt-0.5 font-mono text-xs tabular-nums text-ink">{value}</div>
            </div>
          ))}
        </div>
      ) : null}

      {span.kind === 'compaction' ? (
        <div className="mt-4 rounded-lg border border-line px-2.5 py-1.5">
          <div className="text-[10px] text-ink-faint">{t('activity.attempts')}</div>
          <div className="mt-0.5 font-mono text-xs tabular-nums text-ink">{span.attemptCount ?? '—'}</div>
        </div>
      ) : null}

      {attributeSections.p0.length > 0 ? (
        <div className="mt-4">
          <h4 className="text-xs font-semibold text-ink">{t('activity.traceAttributes')}</h4>
          <TraceAttributeList rows={attributeSections.p0} />
        </div>
      ) : null}
      <TracePayloadSection span={span} onOpenMessage={onOpenMessage} />
      <details className="mt-4 rounded-xl border border-line px-3 py-2.5">
        <summary className="cursor-pointer text-xs font-semibold text-ink">{t('activity.traceDetails')}</summary>
        <dl className="mt-2 divide-y divide-line/70 rounded-xl border border-line px-3">
          {detailFields.map(([label, value]) => (
            <div key={String(label)} className="grid grid-cols-[120px_minmax(0,1fr)] gap-3 py-1.5 text-xs">
              <dt className="text-ink-faint">{label}</dt>
              <dd className="min-w-0 break-all font-mono text-ink-soft">{value}</dd>
            </div>
          ))}
        </dl>
        {attributeSections.p1.length > 0 ? <TraceAttributeList rows={attributeSections.p1} /> : null}
      </details>
      {span.errorMessage ? (
        <div className="mt-4 rounded-xl bg-status-danger-soft p-3 text-xs text-status-danger-ink">
          <div className="font-semibold">{span.errorCode ?? t('activity.error')}</div>
          <p className="mt-1 whitespace-pre-wrap">{span.errorMessage}</p>
        </div>
      ) : null}
    </div>
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
    <section className="mt-4" aria-label={t('activity.payloads.title')}>
      <h4 className="text-xs font-semibold text-ink">{t('activity.payloads.title')}</h4>
      <p className="mt-1 text-[11px] leading-5 text-ink-faint">
        {t('activity.payloads.loadHint')}
      </p>
      <div className="mt-2 grid gap-2">
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
        className="flex w-full items-center gap-2 rounded-xl border border-line bg-paper px-3 py-2 text-xs font-medium text-clay transition-colors hover:bg-clay-soft focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-clay/35"
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
      <div data-payload-slot={slot} className="rounded-xl border border-line bg-paper px-3 py-2">
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
        className="flex w-full items-center gap-2 rounded-xl border border-line bg-paper px-3 py-2 text-xs font-medium text-ink transition-colors hover:bg-paper-hover focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-clay/35"
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
      {localizeTraceValue(status, t)}
    </span>
  )
}

function TraceAttributeList({ rows }: { rows: TraceAttributeRow[] }) {
  const { t } = useTranslation()
  return (
    <dl className="mt-2 divide-y divide-line/60 rounded-xl border border-line px-3">
      {rows.map((row) => (
        <div key={row.key} className="grid grid-cols-[150px_minmax(0,1fr)] gap-3 py-1.5 text-xs">
          <dt className="break-all text-ink-faint">{t(`activity.traceFields.${row.key}`, { defaultValue: row.key })}</dt>
          <dd className="min-w-0 break-all font-mono text-ink-soft">{localizeTraceAttributeValue(row, t)}</dd>
        </div>
      ))}
    </dl>
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
