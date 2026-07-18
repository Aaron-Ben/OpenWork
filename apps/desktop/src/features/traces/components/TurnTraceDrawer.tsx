import { useEffect, useMemo, useRef, useState } from 'react'
import { Bot, Clock3, Wrench, X } from 'lucide-react'
import { AnimatePresence, motion } from 'motion/react'
import { useTranslation } from 'react-i18next'

import { coreCommands } from '../../../bridge/commands'
import type { RuntimeTraceSpan } from '../../../bridge/compat'
import { resolveErrorMessage } from '../../../utils/commandError'
import { shouldPollTrace, type TraceListItem } from '../traceViewModel'
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
  const [spans, setSpans] = useState<RuntimeTraceSpan[] | null>(null)
  const [selectedSpanId, setSelectedSpanId] = useState<string | null>(null)
  const [error, setError] = useState<string | null>(null)
  const drawerRef = useRef<HTMLElement>(null)
  const previousFocus = useRef<HTMLElement | null>(null)
  const requestGeneration = useRef(0)

  useEffect(() => {
    previousFocus.current = document.activeElement as HTMLElement | null
    drawerRef.current?.focus()
    return () => previousFocus.current?.focus()
  }, [])

  useEffect(() => {
    const generation = ++requestGeneration.current
    let active = true
    setSpans(null)
    setError(null)
    void coreCommands.getTrace(turnId)
      .then((value) => {
        if (!active || generation !== requestGeneration.current) return
        setSpans(value)
        const initial = initialProviderCallId
          ? value.find((span) => span.providerCallId === initialProviderCallId)
          : null
        setSelectedSpanId(initial?.id ?? value[0]?.id ?? null)
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

  const shouldPoll = shouldPollTrace(summary?.status, spans ?? [])
  useEffect(() => {
    if (!shouldPoll) return
    const generation = requestGeneration.current
    const timer = window.setInterval(() => {
      void coreCommands.getTrace(turnId)
        .then((value) => {
          if (generation !== requestGeneration.current) return
          setSpans(value)
          setError(null)
          setSelectedSpanId((current) =>
            current && value.some((span) => span.id === current)
              ? current
              : value[0]?.id ?? null,
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
      (sum, span) => sum + (span.inputTokens ?? 0) + (span.outputTokens ?? 0),
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

function SpanDetail({ span }: { span: RuntimeTraceSpan }) {
  const { t } = useTranslation()
  const duration = span.endedAt
    ? Math.max(0, Date.parse(span.endedAt) - Date.parse(span.startedAt))
    : Math.max(0, Date.now() - Date.parse(span.startedAt))
  const fields = [
    [t('activity.statusLabel'), span.status],
    [t('activity.duration'), formatDuration(duration)],
    [t('activity.startedAt'), span.startedAt],
    [t('activity.endedAt'), span.endedAt ?? '—'],
    [t('activity.attempts'), span.attemptCount ?? '—'],
    [t('activity.inputTokens'), span.inputTokens ?? '—'],
    [t('activity.outputTokens'), span.outputTokens ?? '—'],
    [t('activity.cachedTokens'), span.cachedInputTokens ?? '—'],
    [t('activity.permissionWait'), span.permissionWaitMs == null ? '—' : formatDuration(span.permissionWaitMs)],
    [t('activity.providerRequestId'), span.providerRequestId ?? '—'],
  ]
  return (
    <div>
      <div className="flex items-center gap-2">
        {span.kind === 'model_call' ? <Bot size={18} /> : <Wrench size={18} />}
        <h3 className="min-w-0 truncate text-sm font-semibold text-ink">
          {span.kind === 'model_call'
            ? span.resolvedModelName ?? t('activity.modelCall')
            : span.resolvedToolName ?? span.requestedToolName ?? t('activity.toolCall')}
        </h3>
      </div>
      <dl className="mt-5 grid gap-3">
        {fields.map(([label, value]) => (
          <div key={String(label)} className="grid grid-cols-[130px_minmax(0,1fr)] gap-3 border-b border-line pb-2 text-xs">
            <dt className="text-ink-faint">{label}</dt>
            <dd className="min-w-0 break-all font-mono text-ink-soft">{value}</dd>
          </div>
        ))}
      </dl>
      {span.errorMessage ? (
        <div className="mt-5 rounded-xl bg-status-danger-soft p-3 text-xs text-status-danger-ink">
          <div className="font-semibold">{span.errorCode ?? t('activity.error')}</div>
          <p className="mt-1 whitespace-pre-wrap">{span.errorMessage}</p>
        </div>
      ) : null}
    </div>
  )
}
