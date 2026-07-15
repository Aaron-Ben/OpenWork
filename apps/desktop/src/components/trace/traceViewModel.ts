import type { TraceSpan, TurnTraceSummary } from '../../type/trace'

export interface TraceTreeNode {
  span: TraceSpan
  children: TraceTreeNode[]
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
