import type { RuntimeTraceSpan, RuntimeTraceSummary } from '../../bridge/compat'

export type TraceStatusFilter =
  | 'all'
  | 'running'
  | 'completed'
  | 'failed'
  | 'cancelled'
  | 'interrupted'

export interface TraceSessionContext {
  title: string | null
  workingDirectory: string
}

export interface TraceListItem extends RuntimeTraceSummary {
  title: string
  workingDirectory: string
  durationMs: number | null
}

export interface TraceModelNode {
  span: RuntimeTraceSpan
  children: RuntimeTraceSpan[]
}

export interface TraceTree {
  models: TraceModelNode[]
  orphans: RuntimeTraceSpan[]
}

export interface WaterfallRow {
  span: RuntimeTraceSpan
  leftPercent: number
  widthPercent: number
  durationMs: number
}

export function buildTraceListItems(
  summaries: RuntimeTraceSummary[],
  sessions: Record<string, TraceSessionContext>,
  now = Date.now(),
): TraceListItem[] {
  return summaries.map((summary) => {
    const session = sessions[summary.sessionId]
    const start = Date.parse(summary.startedAt)
    const end = summary.endedAt ? Date.parse(summary.endedAt) : now
    return {
      ...summary,
      title: session?.title?.trim() || summary.sessionId,
      workingDirectory: session?.workingDirectory ?? '',
      durationMs: Number.isFinite(start) && Number.isFinite(end) ? Math.max(0, end - start) : null,
    }
  })
}

export function filterTraceListItems(
  items: TraceListItem[],
  query: string,
  status: TraceStatusFilter,
): TraceListItem[] {
  const normalized = query.trim().toLowerCase()
  return items.filter((item) => {
    if (status !== 'all' && item.status !== status) return false
    if (!normalized) return true
    return [
      item.turnId,
      item.sessionId,
      item.resolvedModelName,
      item.title,
      item.workingDirectory,
    ].some((value) => value.toLowerCase().includes(normalized))
  })
}

export function buildTraceTree(spans: RuntimeTraceSpan[]): TraceTree {
  const models = spans
    .filter((span) => span.kind === 'model_call')
    .sort((left, right) => left.sequence - right.sequence)
    .map((span) => ({ span, children: [] as RuntimeTraceSpan[] }))
  const modelById = new Map(models.map((node) => [node.span.id, node]))
  const orphans: RuntimeTraceSpan[] = []

  for (const span of spans) {
    if (span.kind !== 'tool_call') continue
    const parent = span.parentSpanId ? modelById.get(span.parentSpanId) : undefined
    if (parent) parent.children.push(span)
    else orphans.push(span)
  }
  for (const model of models) {
    model.children.sort((left, right) => left.sequence - right.sequence)
  }
  orphans.sort((left, right) => left.sequence - right.sequence)
  return { models, orphans }
}

export function buildWaterfallRows(
  spans: RuntimeTraceSpan[],
  now = Date.now(),
): WaterfallRow[] {
  const parsed = spans.map((span) => {
    const start = Date.parse(span.startedAt)
    const end = span.endedAt ? Date.parse(span.endedAt) : now
    return { span, start, end: Math.max(start, end) }
  }).filter(({ start, end }) => Number.isFinite(start) && Number.isFinite(end))
  if (parsed.length === 0) return []

  const rangeStart = Math.min(...parsed.map((item) => item.start))
  const rangeEnd = Math.max(...parsed.map((item) => item.end))
  const range = Math.max(1, rangeEnd - rangeStart)
  return parsed.map(({ span, start, end }) => ({
    span,
    leftPercent: ((start - rangeStart) / range) * 100,
    widthPercent: Math.max(0.75, ((end - start) / range) * 100),
    durationMs: Math.max(0, end - start),
  }))
}

export function shouldPollTrace(
  summaryStatus: string | null | undefined,
  spans: RuntimeTraceSpan[],
): boolean {
  return summaryStatus === 'running'
    || spans.some((span) => span.status === 'running' || span.endedAt === null)
}
