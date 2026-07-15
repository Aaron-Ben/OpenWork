import { useEffect, useMemo, useState } from 'react'
import { Activity, AlertCircle, Bot, Clock3, RotateCcw, ShieldCheck, Wrench, X } from 'lucide-react'
import { motion } from 'motion/react'
import { useTranslation } from 'react-i18next'

import { sessionsApi } from '../../api/sessions'
import type { TraceSpan, TurnTrace } from '../../type/trace'
import { resolveErrorMessage } from '../../utils/commandError'
import { buildTraceTree, formatDuration, formatTraceSummary, type TraceTreeNode } from './traceViewModel'

export function TurnTracePanel({ turnId, onClose }: { turnId: string; onClose: () => void }) {
  const { i18n, t } = useTranslation()
  const [trace, setTrace] = useState<TurnTrace | null>(null)
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    let active = true
    setTrace(null)
    setError(null)
    void sessionsApi
      .traceTurn(turnId)
      .then((value) => active && setTrace(value))
      .catch((reason) => active && setError(resolveErrorMessage(reason)))
    return () => {
      active = false
    }
  }, [turnId])

  const tree = useMemo(() => buildTraceTree(trace?.spans ?? []), [trace])

  return (
    <motion.aside
      initial={{ opacity: 0, x: 24 }}
      animate={{ opacity: 1, x: 0 }}
      exit={{ opacity: 0, x: 24 }}
      transition={{ duration: 0.2, ease: 'easeOut' }}
      className="absolute inset-y-0 right-0 z-30 flex w-[min(440px,92vw)] flex-col border-l border-line bg-paper shadow-[-18px_0_45px_rgba(20,20,19,0.08)]"
      aria-label={t('trace.title')}
    >
      <header className="flex h-[56px] shrink-0 items-center justify-between border-b border-line px-4">
        <div className="flex items-center gap-2 font-sans font-medium text-ink">
          <Activity size={17} className="text-clay" />
          {t('trace.title')}
        </div>
        <button type="button" onClick={onClose} aria-label={t('trace.close')} className="rounded-lg p-1.5 text-ink-faint hover:bg-paper-hover hover:text-ink">
          <X size={18} />
        </button>
      </header>

      <div className="min-h-0 flex-1 overflow-auto p-4">
        {!trace && !error ? <p className="font-sans text-sm text-ink-faint">{t('trace.loading')}</p> : null}
        {error ? <p className="rounded-xl bg-clay-soft p-3 font-sans text-sm text-ink">{error}</p> : null}
        {trace ? (
          <>
            <section className="rounded-2xl border border-line bg-surface p-4">
              <div className="font-sans text-sm font-medium text-ink">{trace.summary.model ?? t('trace.unknownModel')}</div>
              <div className="mt-2 font-sans text-xs leading-5 text-ink-faint">
                {formatTraceSummary(trace.summary, i18n.language)}
              </div>
              <div className="mt-3 grid grid-cols-3 gap-2 font-sans text-xs">
                <Metric icon={<Clock3 size={14} />} label={t('trace.duration')} value={formatDuration(trace.summary.durationMs, i18n.language)} />
                <Metric icon={<RotateCcw size={14} />} label={t('trace.retries')} value={String(trace.summary.retryCount)} />
                <Metric icon={<AlertCircle size={14} />} label={t('trace.errors')} value={String(trace.summary.errorCount)} />
              </div>
            </section>
            <section className="mt-5">
              <h3 className="mb-2 font-sans text-xs font-medium uppercase tracking-wide text-ink-faint">{t('trace.timeline')}</h3>
              <div className="space-y-1">{tree.map((node) => <TraceNode key={node.span.spanId} node={node} depth={0} />)}</div>
            </section>
          </>
        ) : null}
      </div>
    </motion.aside>
  )
}

function Metric({ icon, label, value }: { icon: React.ReactNode; label: string; value: string }) {
  return <div className="rounded-xl bg-paper-hover px-2.5 py-2"><div className="flex items-center gap-1 text-ink-faint">{icon}{label}</div><div className="mt-1 font-medium text-ink">{value}</div></div>
}

function TraceNode({ node, depth }: { node: TraceTreeNode; depth: number }) {
  const { i18n, t } = useTranslation()
  const span = node.span
  return (
    <div>
      <div className="rounded-xl px-2 py-2 font-sans hover:bg-paper-hover" style={{ marginLeft: Math.min(depth, 4) * 14 }}>
        <div className="flex items-center gap-2 text-sm text-ink">
          <SpanIcon span={span} />
          <span className="min-w-0 flex-1 truncate">{spanTitle(span, t)}</span>
          <span className="text-xs tabular-nums text-ink-faint">{span.durationMs == null ? '—' : formatDuration(span.durationMs, i18n.language)}</span>
          <span className={`size-2 rounded-full ${statusColor(span.status)}`} title={span.status} />
        </div>
        {span.errorMessage ? <p className="mt-1 line-clamp-3 pl-6 text-xs text-clay">{span.errorMessage}</p> : null}
      </div>
      {node.children.map((child) => <TraceNode key={child.span.spanId} node={child} depth={depth + 1} />)}
    </div>
  )
}

function SpanIcon({ span }: { span: TraceSpan }) {
  if (span.spanKind === 'tool_run') return <Wrench size={14} className="text-ink-faint" />
  if (span.spanKind === 'approval') return <ShieldCheck size={14} className="text-ink-faint" />
  if (span.spanKind === 'model_attempt' || span.spanKind === 'transport_attempt') return <Bot size={14} className="text-ink-faint" />
  if (span.spanKind === 'recovery') return <RotateCcw size={14} className="text-ink-faint" />
  return <Activity size={14} className="text-ink-faint" />
}

function spanTitle(span: TraceSpan, t: (key: string, options?: Record<string, unknown>) => string): string {
  const toolName = typeof span.attributes.toolName === 'string' ? span.attributes.toolName : null
  if (span.spanKind === 'tool_run' && toolName) return t('trace.toolRun', { name: toolName })
  return t(`trace.kind.${span.spanKind}`)
}

function statusColor(status: TraceSpan['status']): string {
  if (status === 'failed' || status === 'denied') return 'bg-clay'
  if (status === 'running' || status === 'waiting') return 'bg-amber-500'
  if (status === 'succeeded') return 'bg-emerald-500'
  return 'bg-ink-faint'
}
