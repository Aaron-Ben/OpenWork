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

export interface TraceAttributeRow {
  key: string
  value: string
}

export interface TraceAttributeSections {
  p0: TraceAttributeRow[]
  p1: TraceAttributeRow[]
}

export interface TraceAttempt {
  index: number
  status: string
  durationMs?: number
  errorCode?: string
  errorPhase?: string
  deliveryState?: string
  httpStatus?: number
  providerCode?: string
  providerRequestId?: string
  retryDelayMs?: number
}

export const TRACE_ATTRIBUTE_KEYS = [
  'schemaVersion', 'modelCallIndex', 'requestBuildMs', 'ttftMs', 'streamMs',
  'finishReason', 'responseId', 'actualModel', 'errorPhase', 'deliveryState',
  'httpStatus', 'providerCode', 'attempts', 'inputBytes', 'validationMs',
  'permissionPolicy', 'permissionDecision', 'permissionDecisionSource', 'executionMs',
  'outputBytes', 'outputLines', 'artifactCount', 'errorRetryable', 'resultPersisted',
  'resultPersistMs', 'resultPersistErrorCode', 'appVersion', 'requestMessageCount',
  'requestSystemMessageCount', 'requestUserMessageCount', 'requestAssistantMessageCount',
  'requestToolMessageCount', 'requestContentBytes', 'requestEstimatedSystemContextTokens',
  'requestEstimatedConversationTokens', 'requestEstimatedToolSurfaceTokens',
  'requestEstimatedInputTokens', 'toolDefinitionCount',
  'toolDefinitionBytes', 'maxOutputTokens', 'thinkingMode', 'responseTextBytes',
  'responseReasoningBytes', 'responseToolCallCount', 'responseToolArgumentsBytes',
  'inputTopLevelKeyCount', 'outputTruncated', 'artifactTypes', 'progressEventCount',
] as const

const TRACE_DURATION_KEYS = new Set(['requestBuildMs', 'ttftMs', 'streamMs', 'validationMs', 'executionMs', 'resultPersistMs'])
const TRACE_BYTE_KEYS = new Set(['inputBytes', 'outputBytes', 'requestContentBytes', 'toolDefinitionBytes', 'responseTextBytes', 'responseReasoningBytes', 'responseToolArgumentsBytes'])
const TRACE_P1_KEYS = new Set([
  'appVersion', 'requestMessageCount', 'requestSystemMessageCount',
  'requestUserMessageCount', 'requestAssistantMessageCount', 'requestToolMessageCount',
  'requestContentBytes', 'requestEstimatedSystemContextTokens',
  'requestEstimatedConversationTokens', 'requestEstimatedToolSurfaceTokens', 'requestEstimatedInputTokens',
  'toolDefinitionCount', 'toolDefinitionBytes', 'maxOutputTokens',
  'thinkingMode', 'responseTextBytes', 'responseReasoningBytes', 'responseToolCallCount',
  'responseToolArgumentsBytes', 'inputTopLevelKeyCount', 'outputTruncated', 'artifactTypes',
  'progressEventCount',
])

export function buildTraceAttributeRows(span: RuntimeTraceSpan): TraceAttributeRow[] {
  const rows: TraceAttributeRow[] = []
  for (const key of TRACE_ATTRIBUTE_KEYS) {
    const value = span.attributes[key]
    if (value == null) continue
    if (key === 'attempts' && Array.isArray(value)) {
      rows.push({ key, value: String(value.length) })
      continue
    }
    if (key === 'artifactTypes' && Array.isArray(value)) {
      rows.push({ key, value: value.filter((item): item is string => typeof item === 'string').join(', ') || '—' })
      continue
    }
    if (typeof value === 'number') {
      rows.push({
        key,
        value: TRACE_DURATION_KEYS.has(key)
          ? `${value} ms`
          : TRACE_BYTE_KEYS.has(key)
            ? `${value} B`
            : String(value),
      })
      continue
    }
    if (typeof value === 'string' || typeof value === 'boolean') {
      rows.push({ key, value: String(value) })
    }
  }
  return rows
}

export function buildTraceAttributeSections(span: RuntimeTraceSpan): TraceAttributeSections {
  const rows = buildTraceAttributeRows(span)
  return {
    p0: rows.filter((row) => !TRACE_P1_KEYS.has(row.key)),
    p1: rows.filter((row) => TRACE_P1_KEYS.has(row.key)),
  }
}

export function readTraceAttempts(span: RuntimeTraceSpan): TraceAttempt[] {
  const value = span.attributes.attempts
  if (!Array.isArray(value)) return []
  return value.flatMap((candidate) => {
    if (!candidate || typeof candidate !== 'object') return []
    const record = candidate as Record<string, unknown>
    if (typeof record.index !== 'number' || typeof record.status !== 'string') return []
    const attempt: TraceAttempt = { index: record.index, status: record.status }
    for (const key of ['durationMs', 'httpStatus', 'retryDelayMs'] as const) {
      if (typeof record[key] === 'number') attempt[key] = record[key]
    }
    for (const key of ['errorCode', 'errorPhase', 'deliveryState', 'providerCode', 'providerRequestId'] as const) {
      if (typeof record[key] === 'string') attempt[key] = record[key]
    }
    return [attempt]
  })
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
