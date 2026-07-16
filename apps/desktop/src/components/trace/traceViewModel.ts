import type { TraceSpan, TurnTraceSummary } from '../../type/trace'

export interface TraceTreeNode {
  span: TraceSpan
  children: TraceTreeNode[]
}

export interface FlatTraceTreeNode extends TraceTreeNode {
  depth: number
}

export interface WaterfallSegment {
  leftPercent: number
  widthPercent: number
}

export interface WaterfallTick {
  valueMs: number
  leftPercent: number
}

export function buildTraceTree(spans: TraceSpan[]): TraceTreeNode[] {
  const nodes = new Map<string, TraceTreeNode>()
  for (const span of spans) nodes.set(span.spanId, { span, children: [] })

  const roots: TraceTreeNode[] = []
  for (const node of nodes.values()) {
    const parent = node.span.parentSpanId ? nodes.get(node.span.parentSpanId) : undefined
    if (parent && parent !== node) parent.children.push(node)
    else roots.push(node)
  }

  const sort = (items: TraceTreeNode[]) => {
    items.sort(
      (left, right) =>
        left.span.startedAt - right.span.startedAt ||
        left.span.spanId.localeCompare(right.span.spanId),
    )
    for (const item of items) sort(item.children)
  }
  sort(roots)
  return roots
}

export function flattenTraceTree(
  roots: TraceTreeNode[],
  depth = 0,
): FlatTraceTreeNode[] {
  return roots.flatMap((node) => [
    { ...node, depth },
    ...flattenTraceTree(node.children, depth + 1),
  ])
}

export function filterVisibleTraceRows(
  rows: FlatTraceTreeNode[],
  collapsedSpanIds: ReadonlySet<string>,
): FlatTraceTreeNode[] {
  const visible: FlatTraceTreeNode[] = []
  let collapsedAncestorDepth: number | null = null

  for (const row of rows) {
    if (collapsedAncestorDepth != null) {
      if (row.depth > collapsedAncestorDepth) continue
      collapsedAncestorDepth = null
    }

    visible.push(row)
    if (row.children.length > 0 && collapsedSpanIds.has(row.span.spanId)) {
      collapsedAncestorDepth = row.depth
    }
  }

  return visible
}

export function calculateWaterfallSegment(
  span: TraceSpan,
  turnStartedAt: number,
  turnDurationMs: number,
): WaterfallSegment {
  const duration = Math.max(1, turnDurationMs)
  const start = Math.max(0, span.startedAt - turnStartedAt)
  const effectiveEnd = span.endedAt ?? turnStartedAt + duration
  const spanDuration = Math.max(0, effectiveEnd - span.startedAt)
  const leftPercent = clamp((start / duration) * 100, 0, 100)
  const widthPercent = clamp((spanDuration / duration) * 100, 0.6, 100 - leftPercent)
  return { leftPercent, widthPercent }
}

export function buildWaterfallTicks(
  turnDurationMs: number,
  zoom: number,
): WaterfallTick[] {
  const duration = Math.max(1, turnDurationMs)
  const targetTickCount = Math.max(6, Math.min(30, Math.round(12 * Math.max(1, zoom))))
  const rawStep = Math.max(1, duration / targetTickCount)
  const magnitude = 10 ** Math.floor(Math.log10(rawStep))
  const normalized = rawStep / magnitude
  const factor = normalized < 1.5 ? 1 : normalized < 3 ? 2 : normalized < 7 ? 5 : 10
  const step = Math.max(1, factor * magnitude)
  const ticks: WaterfallTick[] = []

  for (let value = 0; value <= duration; value += step) {
    ticks.push({
      valueMs: Math.round(value),
      leftPercent: (value / duration) * 100,
    })
  }

  return ticks
}

function clamp(value: number, minimum: number, maximum: number): number {
  return Math.min(Math.max(value, minimum), Math.max(minimum, maximum))
}

export function formatTraceSummary(summary: TurnTraceSummary, locale: string): string {
  const duration = formatDuration(summary.durationMs, locale)
  const tokens = new Intl.NumberFormat(locale).format(summary.inputTokens + summary.outputTokens)
  if (locale.startsWith('zh-TW')) {
    return `執行 ${summary.stepCount} 步 · 模型 ${summary.modelAttemptCount} 次 · 工具 ${summary.toolRunCount} 次 · ${duration} · ${tokens} Tokens${summary.retryCount > 0 ? ` · 重試 ${summary.retryCount} 次` : ''}${summary.errorCount > 0 ? ` · 錯誤 ${summary.errorCount} 處` : ''}${summary.recovered ? ' · 已恢復' : ''}`
  }
  if (locale.startsWith('zh')) {
    return `运行 ${summary.stepCount} 步 · 模型 ${summary.modelAttemptCount} 次 · 工具 ${summary.toolRunCount} 次 · ${duration} · ${tokens} Tokens${summary.retryCount > 0 ? ` · 重试 ${summary.retryCount} 次` : ''}${summary.errorCount > 0 ? ` · 错误 ${summary.errorCount} 处` : ''}${summary.recovered ? ' · 已恢复' : ''}`
  }
  return `${summary.stepCount} steps · ${summary.modelAttemptCount} model calls · ${summary.toolRunCount} tools · ${duration} · ${tokens} tokens${summary.retryCount > 0 ? ` · ${summary.retryCount} retries` : ''}${summary.errorCount > 0 ? ` · ${summary.errorCount} errors` : ''}${summary.recovered ? ' · recovered' : ''}`
}

export function formatDuration(durationMs: number, locale: string): string {
  if (durationMs < 1_000) return `${Math.max(0, durationMs)} ms`
  const seconds = durationMs / 1_000
  const rendered = new Intl.NumberFormat(locale, { maximumFractionDigits: 1 }).format(seconds)
  if (locale.startsWith('zh-TW')) return `${rendered} 秒`
  if (locale.startsWith('zh')) return `${rendered} 秒`
  return `${rendered} s`
}
