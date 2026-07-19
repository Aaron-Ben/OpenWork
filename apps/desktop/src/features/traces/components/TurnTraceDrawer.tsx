import { useEffect, useMemo, useRef, useState } from 'react'
import type { TFunction } from 'i18next'
import { Bot, Clock3, Wrench, X } from 'lucide-react'
import { AnimatePresence, motion } from 'motion/react'
import { useTranslation } from 'react-i18next'

import { coreCommands } from '../../../bridge/commands'
import type { RuntimeTraceSpan, RuntimeTurnTrace } from '../../../bridge/compat'
import { formatBeijingDateTime } from '../../../lib/dateTime'
import { resolveErrorMessage } from '../../../utils/commandError'
import {
  buildTraceAttributeSections,
  readTraceAttempts,
  shouldPollTrace,
  type TraceAttributeRow,
  type TraceListItem,
} from '../traceViewModel'
import { formatDuration } from './TraceList'
import { TraceTimeline } from './TraceTimeline'

interface TurnTraceDrawerProps {
  turnId: string
  summary?: TraceListItem | null
  initialProviderCallId?: string
  onClose: () => void
}

export function TurnTraceDrawer({
  turnId,
  summary,
  initialProviderCallId,
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
    void coreCommands.getTrace(turnId)
      .then((value) => {
        if (!active || generation !== requestGeneration.current) return
        setTrace(value)
        const initial = initialProviderCallId
          ? value.spans.find((span) => span.providerCallId === initialProviderCallId)
          : null
        setSelectedSpanId(initial?.id ?? value.spans[0]?.id ?? null)
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
  }, [initialProviderCallId, turnId])

  const shouldPoll = shouldPollTrace(summary?.status ?? trace?.summary.status, spans ?? [])
  useEffect(() => {
    if (!shouldPoll) return
    const generation = requestGeneration.current
    const timer = window.setInterval(() => {
      void coreCommands.getTrace(turnId)
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
  }, [shouldPoll, turnId])

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
          className="absolute inset-y-0 right-0 flex w-[min(880px,96vw)] flex-col border-l border-line bg-paper shadow-[-18px_0_45px_rgba(20,20,19,0.12)] outline-none max-[640px]:w-full"
          onKeyDown={(event) => { if (event.key === 'Escape') onClose() }}
        >
          <header className="border-b border-line px-5 py-4">
            <div className="flex items-start gap-4">
              <div className="min-w-0 flex-1">
                <h2 className="truncate text-base font-semibold text-ink">
                  {summary?.title ?? t('activity.traceDetail')}
                </h2>
                <p className="mt-1 truncate font-mono text-[11px] text-ink-faint">{turnId}</p>
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
              {trace ? (
                <SummaryPill label={t('activity.traceCompleteness', {
                  state: t(`activity.completeness.${trace.completeness.state}`),
                  captured: trace.completeness.capturedModelCalls + trace.completeness.capturedToolCalls,
                  expected: trace.completeness.expectedModelCalls + trace.completeness.expectedToolCalls,
                })} />
              ) : null}
            </div>
          </header>

          <div className="grid min-h-0 flex-1 grid-cols-[minmax(260px,0.9fr)_minmax(300px,1.1fr)] max-[760px]:grid-cols-1 max-[760px]:overflow-auto">
            <section className="min-h-0 overflow-auto border-r border-line p-4 max-[760px]:border-b max-[760px]:border-r-0" aria-label={t('activity.timeline')}>
              {!spans && !error ? <TraceLoading /> : null}
              {error ? <p className="rounded-xl bg-status-danger-soft p-3 text-sm text-status-danger-ink">{error}</p> : null}
              {spans?.length === 0 ? <p className="text-sm text-ink-faint">{t('activity.noSpans')}</p> : null}
              {spans && spans.length > 0 ? (
                <TraceTimeline spans={spans} selectedSpanId={selectedSpanId} onSelect={(span) => setSelectedSpanId(span.id)} />
              ) : null}
            </section>
            <section className="min-h-0 overflow-auto p-5" aria-label={t('activity.spanDetail')}>
              {selected ? <SpanDetail span={selected} /> : <p className="text-sm text-ink-faint">{t('activity.selectSpan')}</p>}
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

export function SpanDetail({ span }: { span: RuntimeTraceSpan }) {
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
  const fields = [
    [t('activity.startedAt'), formatBeijingDateTime(span.startedAt)],
    [t('activity.endedAt'), span.endedAt ? formatBeijingDateTime(span.endedAt) : '—'],
    [t('activity.attempts'), span.attemptCount ?? '—'],
    [t('activity.permissionWait'), span.permissionWaitMs == null ? '—' : formatDuration(span.permissionWaitMs)],
    [t('activity.providerRequestId'), span.providerRequestId ?? '—'],
  ]
  const attributeSections = buildTraceAttributeSections(span)
  const transportAttempts = readTraceAttempts(span)
  return (
    <div>
      <div className="flex items-center gap-3">
        <span className="grid size-9 shrink-0 place-items-center rounded-lg bg-paper-hover text-ink-soft">
          {span.kind === 'model_call' ? <Bot size={16} /> : <Wrench size={16} />}
        </span>
        <div className="min-w-0 flex-1">
          <h3 className="truncate text-sm font-semibold text-ink">
            {span.kind === 'model_call'
              ? span.resolvedModelName ?? t('activity.modelCall')
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

      <dl className="mt-4 divide-y divide-line/70 rounded-xl border border-line px-3">
        {fields.map(([label, value]) => (
          <div key={String(label)} className="grid grid-cols-[120px_minmax(0,1fr)] gap-3 py-1.5 text-xs">
            <dt className="text-ink-faint">{label}</dt>
            <dd className="min-w-0 break-all font-mono text-ink-soft">{value}</dd>
          </div>
        ))}
      </dl>

      {attributeSections.p0.length > 0 ? (
        <div className="mt-4">
          <h4 className="text-xs font-semibold text-ink">{t('activity.traceAttributes')}</h4>
          <TraceAttributeList rows={attributeSections.p0} />
        </div>
      ) : null}
      {transportAttempts.length > 0 ? (
        <details className="mt-4 rounded-xl border border-line px-3 py-2.5">
          <summary className="cursor-pointer text-xs font-semibold text-ink">{t('activity.transportAttempts')}</summary>
          <div className="mt-2.5 grid gap-1.5">
            {transportAttempts.map((attempt) => (
              <div key={attempt.index} className="rounded-lg border border-line/70 px-2.5 py-1.5 font-mono text-[11px] leading-5 text-ink-soft">
                <div>{t('activity.transportAttemptSummary', {
                  index: attempt.index,
                  status: localizeTraceValue(attempt.status, t),
                  duration: attempt.durationMs == null ? '—' : `${attempt.durationMs} ms`,
                })}</div>
                {attempt.errorCode ? <div>{traceDetailField(t, 'errorCode', attempt.errorCode)}</div> : null}
                {attempt.errorPhase ? <div>{traceDetailField(t, 'errorPhase', localizeTraceValue(attempt.errorPhase, t))}</div> : null}
                {attempt.deliveryState ? <div>{traceDetailField(t, 'deliveryState', localizeTraceValue(attempt.deliveryState, t))}</div> : null}
                {attempt.httpStatus == null ? null : <div>{traceDetailField(t, 'httpStatus', attempt.httpStatus)}</div>}
                {attempt.providerCode ? <div>{traceDetailField(t, 'providerCode', attempt.providerCode)}</div> : null}
                {attempt.providerRequestId ? <div>{traceDetailField(t, 'providerRequestId', attempt.providerRequestId)}</div> : null}
                {attempt.retryDelayMs == null ? null : <div>{traceDetailField(t, 'retryDelayMs', `${attempt.retryDelayMs} ms`)}</div>}
              </div>
            ))}
          </div>
        </details>
      ) : null}
      {attributeSections.p1.length > 0 ? (
        <details className="mt-4 rounded-xl border border-line px-3 py-2.5">
          <summary className="cursor-pointer text-xs font-semibold text-ink">{t('activity.traceShape')}</summary>
          <TraceAttributeList rows={attributeSections.p1} />
        </details>
      ) : null}
      {span.errorMessage ? (
        <div className="mt-4 rounded-xl bg-status-danger-soft p-3 text-xs text-status-danger-ink">
          <div className="font-semibold">{span.errorCode ?? t('activity.error')}</div>
          <p className="mt-1 whitespace-pre-wrap">{span.errorMessage}</p>
        </div>
      ) : null}
    </div>
  )
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
  'permissionDecision', 'permissionDecisionSource', 'thinkingMode',
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

function traceDetailField(
  t: TFunction,
  field: string,
  value: string | number,
): string {
  return t('activity.traceDetailField', {
    label: t(`activity.traceFields.${field}`, { defaultValue: field }),
    value,
  })
}
