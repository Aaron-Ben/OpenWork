import { useEffect, useMemo, useState } from 'react'
import { Bot, Clock3, Wrench, X } from 'lucide-react'
import { motion } from 'motion/react'

import { runtimeApi } from '../../api/runtime'
import type { RuntimeTraceSpan } from '../../type/runtime'
import { resolveErrorMessage } from '../../utils/commandError'

interface RuntimeTracePanelProps {
  turnId: string
  initialProviderCallId?: string
  onClose: () => void
}

export function RuntimeTracePanel({
  turnId,
  initialProviderCallId,
  onClose,
}: RuntimeTracePanelProps) {
  const [spans, setSpans] = useState<RuntimeTraceSpan[] | null>(null)
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    let active = true
    setSpans(null)
    setError(null)
    void runtimeApi.getTrace(turnId)
      .then((value) => active && setSpans(value))
      .catch((reason) => active && setError(resolveErrorMessage(reason)))
    return () => {
      active = false
    }
  }, [turnId])

  const modelById = useMemo(
    () => new Map((spans ?? []).filter((span) => span.kind === 'model_call').map((span) => [span.id, span])),
    [spans],
  )

  return (
    <motion.aside
      initial={{ opacity: 0, x: 24 }}
      animate={{ opacity: 1, x: 0 }}
      exit={{ opacity: 0, x: 24 }}
      className="absolute inset-y-0 right-0 z-30 flex w-[min(720px,96vw)] flex-col border-l border-line bg-paper shadow-[-18px_0_45px_rgba(20,20,19,0.08)]"
      aria-label="Turn trace"
    >
      <header className="flex min-h-16 items-center gap-3 border-b border-line px-5">
        <div className="min-w-0 flex-1">
          <div className="text-sm font-semibold text-ink">Turn Trace</div>
          <div className="truncate font-mono text-[11px] text-ink-faint">{turnId}</div>
        </div>
        <button type="button" onClick={onClose} className="rounded-lg p-2 hover:bg-paper-hover" aria-label="Close trace">
          <X size={17} />
        </button>
      </header>
      <div className="min-h-0 flex-1 overflow-auto p-4">
        {!spans && !error ? <p className="text-sm text-ink-faint">Loading trace…</p> : null}
        {error ? <p className="rounded-xl bg-status-danger-soft p-3 text-sm text-status-danger-ink">{error}</p> : null}
        {spans?.length === 0 ? <p className="text-sm text-ink-faint">No trace spans were recorded.</p> : null}
        <div className="grid gap-3">
          {spans?.map((span) => {
            const parent = span.parentSpanId ? modelById.get(span.parentSpanId) : null
            const selected = initialProviderCallId === span.providerCallId
            return (
              <article
                key={span.id}
                className={`rounded-xl border p-4 ${selected ? 'border-clay bg-clay-soft' : 'border-line bg-paper-hover'}`}
              >
                <div className="flex items-start gap-3">
                  <div className="mt-0.5 grid size-8 place-items-center rounded-lg bg-paper">
                    {span.kind === 'model_call' ? <Bot size={16} /> : <Wrench size={16} />}
                  </div>
                  <div className="min-w-0 flex-1">
                    <div className="flex flex-wrap items-center gap-2">
                      <span className="text-sm font-semibold text-ink">
                        {span.kind === 'model_call'
                          ? span.resolvedModelName ?? 'Model call'
                          : span.resolvedToolName ?? span.requestedToolName ?? 'Tool call'}
                      </span>
                      <StatusPill status={span.status} />
                    </div>
                    <div className="mt-1 flex flex-wrap gap-x-3 gap-y-1 text-[11px] text-ink-faint">
                      <span>#{span.sequence}</span>
                      <span className="inline-flex items-center gap-1"><Clock3 size={11} />{duration(span)} ms</span>
                      {span.kind === 'model_call' ? (
                        <span>tokens {(span.inputTokens ?? 0) + (span.outputTokens ?? 0)}</span>
                      ) : null}
                      {span.permissionWaitMs != null ? <span>permission {span.permissionWaitMs} ms</span> : null}
                      {parent ? <span>from #{parent.sequence}</span> : null}
                    </div>
                    {span.errorMessage ? (
                      <p className="mt-2 whitespace-pre-wrap text-xs text-status-danger-ink">
                        {span.errorCode ? `${span.errorCode}: ` : ''}{span.errorMessage}
                      </p>
                    ) : null}
                  </div>
                </div>
              </article>
            )
          })}
        </div>
      </div>
    </motion.aside>
  )
}

function duration(span: RuntimeTraceSpan): number {
  if (!span.endedAt) return 0
  return Math.max(0, Date.parse(span.endedAt) - Date.parse(span.startedAt))
}

function StatusPill({ status }: { status: string }) {
  const color = status === 'succeeded'
    ? 'bg-status-success-soft text-status-success'
    : status === 'running'
      ? 'bg-clay-soft text-clay'
      : 'bg-status-danger-soft text-status-danger-ink'
  return <span className={`rounded-full px-2 py-0.5 text-[10px] font-semibold ${color}`}>{status}</span>
}
