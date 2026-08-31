import { AlertCircle, CheckCircle2, Clock3 } from 'lucide-react'
import { useTranslation } from 'react-i18next'

import type { CollabRun, CollabRunEvent } from '@/bridge/collab'

export function TraceDetail({ run, events }: { run: CollabRun; events: CollabRunEvent[] }) {
  const { t } = useTranslation()
  const totalTokens = (run.inputTokens ?? 0)
    + (run.cachedInputTokens ?? 0)
    + (run.cacheCreationInputTokens ?? 0)
    + (run.outputTokens ?? 0)
  return (
    <article className="mx-auto grid max-w-5xl gap-5 p-6">
      <div className="grid gap-2">
        <div className="flex flex-wrap items-center gap-2">
          <h2 className="font-serif text-xl font-semibold">@{run.agentId}</h2>
          <RunStatusBadge status={run.status} />
          {run.outcome ? <span className="rounded-full bg-paper-hover px-2 py-0.5 text-xs text-ink-muted">{run.outcome}</span> : null}
        </div>
        <p className="break-all font-mono text-xs text-ink-faint">{run.id}</p>
        {run.triggerReason ? <p className="text-sm text-ink-muted">{run.triggerReason}</p> : null}
      </div>

      <dl className="grid grid-cols-2 gap-2 sm:grid-cols-3 xl:grid-cols-6">
        <Metric label={t('collab.observability.duration')} value={formatDuration(run.durationMs, t)} />
        <Metric label={t('collab.observability.tokens')} value={formatCount(totalTokens)} />
        <Metric label={t('collab.observability.tools')} value={formatCount(run.toolCalls)} />
        <Metric label={t('collab.observability.inbox')} value={formatCount(run.inboxMessageCount)} />
        <Metric label={t('collab.observability.events')} value={formatCount(run.eventCount)} />
        <Metric label={t('collab.observability.trigger')} value={run.trigger} />
      </dl>

      <dl className="grid gap-x-6 gap-y-2 rounded-2xl border border-line p-4 text-sm sm:grid-cols-2">
        <Property label={t('collab.observability.engine')} value={run.engineId} />
        <Property label={t('collab.observability.configuredModel')} value={run.mainModelId} />
        <Property label={t('collab.observability.observedModel')} value={run.observedModelId ?? '—'} />
        <Property label={t('collab.observability.startedAt')} value={formatDateTime(run.startedAt)} />
      </dl>

      {run.errorMessage ? (
        <div className="rounded-2xl border border-red-200 bg-red-50 p-4 text-sm text-red-700">
          <strong>{run.errorCode ?? t('collab.observability.unknownError')}</strong>
          <p className="mt-1 whitespace-pre-wrap">{run.errorMessage}</p>
        </div>
      ) : null}

      <section className="grid gap-3">
        <h3 className="font-serif text-lg font-semibold">{t('collab.observability.timeline')}</h3>
        {events.length === 0 ? <p className="text-sm text-ink-faint">{t('collab.observability.noEvents')}</p> : null}
        <div className="relative grid gap-3 before:absolute before:bottom-4 before:left-[7px] before:top-4 before:w-px before:bg-line">
          {events.map((event) => <EventRow key={event.id} event={event} />)}
        </div>
      </section>
    </article>
  )
}

export function RunStatusBadge({ status }: { status: string }) {
  const { t } = useTranslation()
  const successful = status === 'completed'
  const running = status === 'running'
  return (
    <span className={`inline-flex items-center gap-1 rounded-full px-2 py-0.5 text-[11px] ${running ? 'bg-blue-50 text-blue-700' : successful ? 'bg-emerald-50 text-emerald-700' : 'bg-red-50 text-red-700'}`}>
      {running ? <Clock3 size={11} /> : successful ? <CheckCircle2 size={11} /> : <AlertCircle size={11} />}
      {t(`collab.observability.status.${status}`, { defaultValue: status })}
    </span>
  )
}

function EventRow({ event }: { event: CollabRunEvent }) {
  const { t } = useTranslation()
  const details = eventSummary(event, t)
  return (
    <article className="relative grid grid-cols-[15px_minmax(0,1fr)] gap-3">
      <span className={`z-10 mt-1.5 size-[15px] rounded-full border-2 border-paper ${event.level === 'error' ? 'bg-red-500' : event.level === 'warning' ? 'bg-amber-500' : 'bg-clay'}`} />
      <div className="rounded-2xl border border-line bg-paper p-3">
        <div className="flex flex-wrap items-start justify-between gap-2">
          <div>
            <strong className="text-sm">{eventLabel(event.kind, event.data, t)}</strong>
            <span className="ml-2 font-mono text-[11px] text-ink-faint">{event.source}</span>
          </div>
          <time className="text-xs text-ink-faint">{formatTime(event.createdAt)}</time>
        </div>
        {details ? <p className="mt-1 text-sm text-ink-muted">{details}</p> : null}
        <EventDetails event={event} />
        {Object.keys(event.data).length > 0 ? (
          <details className="mt-2 text-xs">
            <summary className="cursor-pointer select-none text-ink-faint">{t('collab.observability.rawData')}</summary>
            <pre className="mt-2 max-h-72 overflow-auto rounded-xl bg-paper-hover p-3 font-mono text-[11px] leading-relaxed text-ink-muted">{JSON.stringify(event.data, null, 2)}</pre>
          </details>
        ) : null}
      </div>
    </article>
  )
}

function EventDetails({ event }: { event: CollabRunEvent }) {
  const { t } = useTranslation()
  const properties = eventProperties(event, t)
  if (properties.length === 0) return null
  return (
    <dl className="mt-3 grid gap-x-5 gap-y-2 rounded-xl bg-paper-hover p-3 text-xs sm:grid-cols-2">
      {properties.map(([label, value]) => (
        <div key={label} className="grid grid-cols-[minmax(88px,auto)_minmax(0,1fr)] gap-2">
          <dt className="text-ink-faint">{label}</dt>
          <dd className="min-w-0 break-words font-mono text-ink-muted">{value}</dd>
        </div>
      ))}
    </dl>
  )
}

function Metric({ label, value }: { label: string; value: string }) {
  return <div className="rounded-xl bg-paper-hover p-3"><dt className="text-xs text-ink-faint">{label}</dt><dd className="mt-1 truncate text-sm font-semibold">{value}</dd></div>
}

function Property({ label, value }: { label: string; value: string }) {
  return <div className="grid grid-cols-[120px_minmax(0,1fr)] gap-2"><dt className="text-ink-faint">{label}</dt><dd className="truncate font-mono text-xs">{value}</dd></div>
}

function eventLabel(kind: string, data: Record<string, unknown>, t: ReturnType<typeof useTranslation>['t']): string {
  if (kind === 'triage.completed') {
    return data.actionable
      ? t('collab.observability.event.triageActionable')
      : t('collab.observability.event.triageIgnored')
  }
  if (kind === 'command.completed') {
    const command = nestedString(data, 'response', 'effects', '0', 'type')
      ?? nestedString(data, 'response', 'result', 'type')
      ?? 'unknown'
    return t('collab.observability.event.commandCompleted', { command })
  }
  return t(`collab.observability.event.${kind.replace('.', '_')}`, { defaultValue: kind })
}

function eventSummary(event: CollabRunEvent, t: ReturnType<typeof useTranslation>['t']): string | null {
  const duration = numberValue(event.data.durationMs)
  const model = stringValue(event.data.observedModelId) ?? stringValue(event.data.configuredModelId)
  const reason = stringValue(event.data.reason) ?? stringValue(event.data.errorMessage)
  const parts = [model, duration === null ? null : formatDuration(duration, t), reason]
    .filter((part): part is string => Boolean(part))
  return parts.length > 0 ? parts.join(' · ') : null
}

function eventProperties(
  event: CollabRunEvent,
  t: ReturnType<typeof useTranslation>['t'],
): Array<[string, string]> {
  const data = event.data
  const properties: Array<[string, string | null]> = []
  const add = (label: string, value: string | number | null | undefined) => {
    if (value !== null && value !== undefined && value !== '') properties.push([label, String(value)])
  }

  if (event.kind === 'run.opened') {
    add(t('collab.observability.trigger'), stringValue(data.trigger))
    add(t('collab.observability.detail.room'), stringValue(data.roomId))
    add(t('collab.observability.inbox'), numberValue(data.inboxMessageCount))
  } else if (event.kind === 'triage.started') {
    add(t('collab.observability.configuredModel'), stringValue(data.configuredModelId))
  } else if (event.kind === 'triage.completed') {
    add(t('collab.observability.detail.model'), stringValue(data.modelId))
    add(t('collab.observability.detail.reason'), stringValue(data.reason))
    add(t('collab.observability.detail.latency'), durationValue(data.latencyMs, t))
    add(t('collab.observability.detail.inputTokens'), numberValue(data.inputTokens))
    add(t('collab.observability.detail.outputTokens'), numberValue(data.outputTokens))
  } else if (event.kind === 'engine.started') {
    add(t('collab.observability.configuredModel'), stringValue(data.configuredModelId))
  } else if (event.kind === 'engine.completed') {
    add(t('collab.observability.observedModel'), stringValue(data.observedModelId))
    add(t('collab.observability.duration'), durationValue(data.durationMs, t))
    add(t('collab.observability.detail.responseSize'), byteValue(data.responseBytes))
    add(t('collab.observability.detail.inputTokens'), nestedNumber(data, 'usage', 'inputTokens'))
    add(t('collab.observability.detail.cachedInputTokens'), nestedNumber(data, 'usage', 'cachedInputTokens'))
    add(t('collab.observability.detail.outputTokens'), nestedNumber(data, 'usage', 'outputTokens'))
  } else if (event.kind === 'engine.failed' || event.kind === 'engine.cancelled') {
    add(t('collab.observability.duration'), durationValue(data.durationMs, t))
    add(t('collab.observability.detail.result'), stringValue(data.errorCode))
    add(t('collab.observability.detail.reason'), stringValue(data.errorMessage))
    add(t('collab.observability.detail.retryAfter'), durationValue(data.retryAfterMs, t))
  } else if (event.kind === 'command.completed') {
    add(t('collab.observability.detail.requestId'), stringValue(data.requestId))
    add(
      t('collab.observability.detail.result'),
      nestedString(data, 'response', 'result', 'type')
        ?? nestedString(data, 'response', 'effects', '0', 'type'),
    )
  } else if (event.kind.startsWith('run.')) {
    add(t('collab.observability.detail.outcome'), stringValue(data.outcome))
    add(t('collab.observability.duration'), durationValue(data.durationMs, t))
    add(t('collab.observability.detail.result'), stringValue(data.errorCode))
    add(t('collab.observability.detail.reason'), stringValue(data.errorMessage))
  }

  return properties.filter((entry): entry is [string, string] => entry[1] !== null)
}

function durationValue(value: unknown, t: ReturnType<typeof useTranslation>['t']): string | null {
  const duration = numberValue(value)
  return duration === null ? null : formatDuration(duration, t)
}

function byteValue(value: unknown): string | null {
  const bytes = numberValue(value)
  if (bytes === null) return null
  if (bytes < 1024) return `${bytes} B`
  return `${(bytes / 1024).toFixed(1)} KB`
}

function nestedNumber(value: Record<string, unknown>, ...path: string[]): number | null {
  let current: unknown = value
  for (const key of path) {
    if (!current || typeof current !== 'object' || Array.isArray(current)) return null
    current = (current as Record<string, unknown>)[key]
  }
  return numberValue(current)
}

function nestedString(value: Record<string, unknown>, ...path: string[]): string | null {
  let current: unknown = value
  for (const key of path) {
    if (typeof current !== 'object' || current === null) return null
    current = (current as Record<string, unknown>)[key]
  }
  return stringValue(current)
}

function stringValue(value: unknown): string | null {
  return typeof value === 'string' && value.length > 0 ? value : null
}

function numberValue(value: unknown): number | null {
  return typeof value === 'number' && Number.isFinite(value) ? value : null
}

function formatDuration(milliseconds: number, t: ReturnType<typeof useTranslation>['t']): string {
  if (milliseconds < 1_000) return t('collab.observability.milliseconds', { count: milliseconds })
  if (milliseconds < 60_000) return `${(milliseconds / 1_000).toFixed(1)}s`
  return `${Math.floor(milliseconds / 60_000)}m ${Math.floor((milliseconds % 60_000) / 1_000)}s`
}

function formatCount(value: number): string {
  return new Intl.NumberFormat().format(value)
}

function formatTime(value: string): string {
  return new Intl.DateTimeFormat(undefined, { hour: '2-digit', minute: '2-digit', second: '2-digit' }).format(new Date(value))
}

function formatDateTime(value: string): string {
  return new Intl.DateTimeFormat(undefined, { dateStyle: 'medium', timeStyle: 'medium' }).format(new Date(value))
}
