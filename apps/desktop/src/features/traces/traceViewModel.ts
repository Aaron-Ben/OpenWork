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
  roots: TraceModelNode[]
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
  key: TraceAttributeKey
  value: string
}

export interface TraceAttributeSections {
  p0: TraceAttributeRow[]
  p1: TraceAttributeRow[]
}

export const TRACE_ATTRIBUTE_KEYS = [
  'schemaVersion', 'modelCallIndex', 'temperature', 'topP', 'toolChoice',
  'requestBuildMs', 'ttftMs', 'streamMs',
  'finishReason', 'responseId', 'actualModel', 'errorPhase', 'deliveryState',
  'httpStatus', 'providerCode',
  'permissionPolicy', 'permissionDecision', 'permissionDecisionSource', 'executionMs',
  'artifactCount', 'errorRetryable', 'resultPersisted',
  'requestMessageCount',
  'requestEstimatedSystemContextTokens',
  'requestEstimatedConversationTokens', 'requestEstimatedToolSurfaceTokens',
  'requestEstimatedInputTokens', 'toolDefinitionCount',
  'maxOutputTokens', 'thinkingMode', 'responseToolCallCount',
  'outputTruncated', 'artifactTypes',
  'trigger', 'sourceMessageCount', 'prepareMs', 'summaryMs', 'persistenceMs', 'installMs', 'summaryChars',
  'checkpointId', 'summaryRequestMessageCount', 'summaryEstimatedSystemContextTokens',
  'summaryEstimatedConversationTokens', 'summaryEstimatedToolSurfaceTokens',
  'summaryMaxOutputTokens',
  'conversationTokensBefore', 'conversationTokensAfter', 'reclaimedConversationTokens',
  'triggerEstimatedInputTokens', 'triggerPercent', 'contextWindowTokens', 'thresholdPercent',
  'triggerModelSpanId', 'triggerErrorCode', 'summaryRetryDelayMs',
] as const

export type TraceAttributeKey = typeof TRACE_ATTRIBUTE_KEYS[number]
export type TraceAttributePlacement = RuntimeTraceSpan['kind'] | 'details'

export const TRACE_ATTRIBUTE_PLACEMENT = {
  schemaVersion: 'details',
  modelCallIndex: 'details',
  temperature: 'model_call',
  topP: 'details',
  toolChoice: 'details',
  requestBuildMs: 'details',
  ttftMs: 'details',
  streamMs: 'details',
  finishReason: 'model_call',
  responseId: 'details',
  actualModel: 'details',
  errorPhase: 'details',
  deliveryState: 'details',
  httpStatus: 'details',
  providerCode: 'details',
  permissionPolicy: 'details',
  permissionDecision: 'tool_call',
  permissionDecisionSource: 'details',
  executionMs: 'tool_call',
  artifactCount: 'details',
  errorRetryable: 'details',
  resultPersisted: 'details',
  requestMessageCount: 'details',
  requestEstimatedSystemContextTokens: 'details',
  requestEstimatedConversationTokens: 'details',
  requestEstimatedToolSurfaceTokens: 'details',
  requestEstimatedInputTokens: 'details',
  toolDefinitionCount: 'details',
  maxOutputTokens: 'details',
  thinkingMode: 'details',
  responseToolCallCount: 'details',
  outputTruncated: 'details',
  artifactTypes: 'details',
  trigger: 'compaction',
  sourceMessageCount: 'details',
  prepareMs: 'details',
  summaryMs: 'details',
  persistenceMs: 'details',
  installMs: 'details',
  summaryChars: 'details',
  checkpointId: 'details',
  summaryRequestMessageCount: 'details',
  summaryEstimatedSystemContextTokens: 'details',
  summaryEstimatedConversationTokens: 'details',
  summaryEstimatedToolSurfaceTokens: 'details',
  summaryMaxOutputTokens: 'details',
  conversationTokensBefore: 'compaction',
  conversationTokensAfter: 'compaction',
  reclaimedConversationTokens: 'compaction',
  triggerEstimatedInputTokens: 'details',
  triggerPercent: 'details',
  contextWindowTokens: 'details',
  thresholdPercent: 'details',
  triggerModelSpanId: 'details',
  triggerErrorCode: 'details',
  summaryRetryDelayMs: 'details',
} as const satisfies Record<TraceAttributeKey, TraceAttributePlacement>

const TRACE_DURATION_KEYS = new Set<TraceAttributeKey>([
  'requestBuildMs', 'ttftMs', 'streamMs', 'executionMs', 'prepareMs',
  'summaryMs', 'persistenceMs', 'installMs', 'summaryRetryDelayMs',
])
const TRACE_PERCENT_KEYS = new Set<TraceAttributeKey>(['triggerPercent', 'thresholdPercent'])

export function buildTraceAttributeRows(span: RuntimeTraceSpan): TraceAttributeRow[] {
  const rows: TraceAttributeRow[] = []
  for (const key of TRACE_ATTRIBUTE_KEYS) {
    const value = span.attributes[key]
    if (value == null) continue
    if (key === 'artifactTypes' && Array.isArray(value)) {
      rows.push({ key, value: value.filter((item): item is string => typeof item === 'string').join(', ') || '—' })
      continue
    }
    if (typeof value === 'number') {
      rows.push({
        key,
        value: TRACE_DURATION_KEYS.has(key)
          ? `${value} ms`
          : TRACE_PERCENT_KEYS.has(key)
            ? `${value}%`
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
    p0: rows.filter((row) => TRACE_ATTRIBUTE_PLACEMENT[row.key] === span.kind),
    p1: rows.filter((row) => TRACE_ATTRIBUTE_PLACEMENT[row.key] !== span.kind),
  }
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
      item.traceId,
      item.sessionId,
      item.resolvedModelName,
      item.title,
      item.workingDirectory,
    ].some((value) => value != null && value.toLowerCase().includes(normalized))
  })
}

export interface CompactionHistoryItem {
  span: RuntimeTraceSpan
  /** `manual` | `threshold` | `overflow` | `rewind`, or `unknown` on a Span written before triggers were recorded. */
  trigger: string
  durationMs: number | null
  conversationTokensBefore: number | null
  conversationTokensAfter: number | null
  reclaimedTokens: number | null
  /** Absent for a manual compaction or a rewind, which run outside any Turn. */
  turnId: string | null
}

/**
 * Session compaction history, newest first.
 *
 * The backend already orders by `started_at DESC`; ordering is repeated here so
 * the view does not depend on query order.
 */
export function buildCompactionHistory(
  spans: RuntimeTraceSpan[],
  now = Date.now(),
): CompactionHistoryItem[] {
  return spans
    .filter((span) => span.kind === 'compaction')
    .map((span) => {
      const start = Date.parse(span.startedAt)
      const end = span.endedAt ? Date.parse(span.endedAt) : now
      const before = readNumberAttribute(span, 'conversationTokensBefore')
      const after = readNumberAttribute(span, 'conversationTokensAfter')
      return {
        span,
        trigger: readStringAttribute(span, 'trigger') ?? 'unknown',
        durationMs: Number.isFinite(start) && Number.isFinite(end) ? Math.max(0, end - start) : null,
        conversationTokensBefore: before,
        conversationTokensAfter: after,
        reclaimedTokens: readNumberAttribute(span, 'reclaimedConversationTokens'),
        turnId: span.turnId,
      }
    })
    .sort((left, right) => Date.parse(right.span.startedAt) - Date.parse(left.span.startedAt))
}

function readNumberAttribute(span: RuntimeTraceSpan, key: string): number | null {
  const value = span.attributes[key]
  return typeof value === 'number' ? value : null
}

function readStringAttribute(span: RuntimeTraceSpan, key: string): string | null {
  const value = span.attributes[key]
  return typeof value === 'string' ? value : null
}

export function buildTraceTree(spans: RuntimeTraceSpan[]): TraceTree {
  const models = spans
    .filter((span) => span.kind === 'model_call')
    .sort(compareSpanStart)
    .map((span) => ({ span, children: [] as RuntimeTraceSpan[] }))
  const compactions = spans
    .filter((span) => span.kind === 'compaction')
    .sort(compareSpanStart)
    .map((span) => ({ span, children: [] as RuntimeTraceSpan[] }))
  const nodeById = new Map([...models, ...compactions].map((node) => [node.span.id, node]))
  const orphans: RuntimeTraceSpan[] = []

  for (const span of spans) {
    if (!span.parentSpanId) continue
    const parent = nodeById.get(span.parentSpanId)
    if (parent) {
      parent.children.push(span)
    } else {
      orphans.push(span)
    }
  }
  for (const node of nodeById.values()) {
    node.children.sort(compareSpanStart)
  }
  const roots = [...models, ...compactions]
    .filter((node) => node.span.parentSpanId === null)
    .sort((left, right) => compareSpanStart(left.span, right.span))
  orphans.sort(compareSpanStart)
  return { roots, models, orphans }
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
  return parsed
    .sort((left, right) => compareSpanStart(left.span, right.span))
    .map(({ span, start, end }) => ({
      span,
      leftPercent: ((start - rangeStart) / range) * 100,
      widthPercent: Math.max(0.75, ((end - start) / range) * 100),
      durationMs: Math.max(0, end - start),
    }))
}

function compareSpanStart(left: RuntimeTraceSpan, right: RuntimeTraceSpan): number {
  const leftStartedAt = Date.parse(left.startedAt)
  const rightStartedAt = Date.parse(right.startedAt)
  if (Number.isFinite(leftStartedAt) && Number.isFinite(rightStartedAt)) {
    const byTime = leftStartedAt - rightStartedAt
    if (byTime !== 0) return byTime
  } else {
    const byText = left.startedAt.localeCompare(right.startedAt)
    if (byText !== 0) return byText
  }
  return left.id.localeCompare(right.id)
}

export function shouldPollTrace(
  summaryStatus: string | null | undefined,
  spans: RuntimeTraceSpan[],
): boolean {
  return summaryStatus === 'running'
    || spans.some((span) => span.status === 'running' || span.endedAt === null)
}
