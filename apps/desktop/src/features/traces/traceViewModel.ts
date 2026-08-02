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
  'permissionPolicy', 'permissionMode', 'permissionModeOrigin', 'permissionDecision', 'permissionDecisionSource',
  'readonlyProofKey', 'permissionRuleId', 'permissionRuleScope', 'executionMs',
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
  permissionMode: 'details',
  permissionModeOrigin: 'details',
  permissionDecision: 'tool_call',
  permissionDecisionSource: 'details',
  readonlyProofKey: 'details',
  permissionRuleId: 'details',
  permissionRuleScope: 'details',
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

export interface WaterfallRange {
  startMs: number
  endMs: number
}

interface SpanTiming {
  span: RuntimeTraceSpan
  start: number
  end: number
}

function parseSpanTimings(spans: RuntimeTraceSpan[], now: number): SpanTiming[] {
  return spans
    .map((span) => {
      const start = Date.parse(span.startedAt)
      const end = span.endedAt ? Date.parse(span.endedAt) : now
      return { span, start, end: Math.max(start, end) }
    })
    .filter(({ start, end }) => Number.isFinite(start) && Number.isFinite(end))
}

export function buildWaterfallRows(
  spans: RuntimeTraceSpan[],
  now = Date.now(),
): WaterfallRow[] {
  const parsed = parseSpanTimings(spans, now)
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

/** 瀑布的时间范围（刻度尺用），与 buildWaterfallRows 共用同一套解析与边界。 */
export function buildWaterfallRange(
  spans: RuntimeTraceSpan[],
  now = Date.now(),
): WaterfallRange | null {
  const parsed = parseSpanTimings(spans, now)
  if (parsed.length === 0) return null
  return {
    startMs: Math.min(...parsed.map((item) => item.start)),
    endMs: Math.max(...parsed.map((item) => item.end)),
  }
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

export type TraceSortKey =
  | 'startedAt'
  | 'durationMs'
  | 'modelSubmissionCount'
  | 'toolCallCount'
  | 'totalTokens'
  | 'resolvedModelName'

export type TraceSortDirection = 'asc' | 'desc'

export interface TraceSort {
  key: TraceSortKey
  direction: TraceSortDirection
}

/** 默认与后端 `ORDER BY started_at DESC` 一致，首次渲染不改变既有顺序。 */
export const DEFAULT_TRACE_SORT: TraceSort = { key: 'startedAt', direction: 'desc' }

function traceSortValue(item: TraceListItem, key: TraceSortKey): number | string | null {
  switch (key) {
    case 'startedAt': return item.startedAt
    case 'durationMs': return item.durationMs
    case 'modelSubmissionCount': return item.modelSubmissionCount
    case 'toolCallCount': return item.toolCallCount
    case 'totalTokens': return item.totalTokens
    case 'resolvedModelName': return item.resolvedModelName || null
  }
}

/** 客户端排序；空值（运行中的耗时、无模型名）无论方向都沉底，traceId 兜底保证稳定。 */
export function sortTraceListItems(
  items: TraceListItem[],
  sort: TraceSort,
): TraceListItem[] {
  const factor = sort.direction === 'asc' ? 1 : -1
  return [...items].sort((left, right) => {
    const a = traceSortValue(left, sort.key)
    const b = traceSortValue(right, sort.key)
    if (a == null && b == null) return left.traceId.localeCompare(right.traceId)
    if (a == null) return 1
    if (b == null) return -1
    const compared = typeof a === 'string' || typeof b === 'string'
      ? String(a).localeCompare(String(b))
      : a - b
    return compared !== 0 ? compared * factor : left.traceId.localeCompare(right.traceId)
  })
}
