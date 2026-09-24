import type { RuntimeTraceSpan } from '@/bridge/compat'

/**
 * Tool Span 在时间线上的类别（permissions.md §6.4）。前七类是设计规定的；
 * `control_tool` 是 Core 控制工具，`cancelled` 是等待审批时 Turn 被取消，
 * `unknown` 是属性缺失或来自旧版本的 Trace——显示"来源未知"，不留白（§7）。
 */
export type PermissionCategory =
  | 'sandbox_auto'
  | 'sandbox_denied'
  | 'user_approved_escalation'
  | 'user_approved_danger'
  | 'sandbox_unavailable'
  | 'rule_denied'
  | 'user_denied'
  | 'control_tool'
  | 'cancelled'
  | 'unknown'

function stringAttribute(span: RuntimeTraceSpan, key: string): string | null {
  const value = span.attributes[key]
  return typeof value === 'string' ? value : null
}

function hasEscalation(span: RuntimeTraceSpan): boolean {
  const paths = span.attributes.escalationPaths
  return Array.isArray(paths) && paths.length > 0
}

/**
 * 一个 Span 属于哪一类；不是 Tool Span 时返回 `null`。
 *
 * 顺序有意义：没有执行的调用（沙箱不可用、拒绝）先判；执行了却被内核拒绝的调用即使
 * 经过用户批准也归"被沙箱拒绝"，因为回看时要找的是结果。
 */
export function permissionCategory(span: RuntimeTraceSpan): PermissionCategory | null {
  if (span.kind !== 'tool_call') return null
  const decision = stringAttribute(span, 'permissionDecision')
  const source = stringAttribute(span, 'permissionDecisionSource')

  if (source === 'sandbox_unavailable') return 'sandbox_unavailable'
  if (decision === 'cancelled') return 'cancelled'
  if (decision === 'deny' && source === 'user') return 'user_denied'
  if (decision === 'deny' && (source === 'builtin' || source === 'non_interactive')) {
    return 'rule_denied'
  }
  if (decision !== 'allow') return 'unknown'
  if (source === 'control_tool') return 'control_tool'
  if (span.attributes.sandboxDenied === true) return 'sandbox_denied'
  if (source === 'user') {
    if (hasEscalation(span)) return 'user_approved_escalation'
    if (stringAttribute(span, 'dangerMatch')) return 'user_approved_danger'
    return 'unknown'
  }
  return source === 'sandbox' ? 'sandbox_auto' : 'unknown'
}
