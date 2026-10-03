import { describe, expect, it } from 'vitest'

import type { RuntimeTraceSpan } from '@/bridge/compat'
import { permissionCategory } from './permissionCategory'

function toolSpan(attributes: Record<string, unknown>): RuntimeTraceSpan {
  return {
    id: 'span-1',
    traceId: 'trace-1',
    sessionId: 'session-1',
    turnId: 'turn-1',
    parentSpanId: null,
    kind: 'tool_call',
    name: 'bash',
    status: 'succeeded',
    modelId: null,
    resolvedModelName: null,
    providerRequestId: null,
    providerCallId: 'call-1',
    requestedToolName: 'bash',
    resolvedToolName: 'bash',
    attemptCount: null,
    inputTokens: null,
    outputTokens: null,
    cachedInputTokens: null,
    reasoningTokens: null,
    totalTokens: null,
    responseMessageId: null,
    permissionWaitMs: null,
    startedAt: '2026-09-24T10:00:00.000000+08:00',
    endedAt: '2026-09-24T10:00:01.000000+08:00',
    errorCode: null,
    errorMessage: null,
    attributes,
  }
}

describe('permissionCategory', () => {
  /** permissions.md §14.1 的七类，外加控制工具、取消与来源未知。 */
  it.each([
    [{ permissionDecision: 'allow', permissionDecisionSource: 'sandbox' }, 'sandbox_auto'],
    [{ permissionDecision: 'allow', permissionDecisionSource: 'sandbox', sandboxDenied: true }, 'sandbox_denied'],
    [
      { permissionDecision: 'allow', permissionDecisionSource: 'user', escalationPaths: [{ path: '/w/.git', access: 'write', scope: 'subtree' }] },
      'user_approved_escalation',
    ],
    [{ permissionDecision: 'allow', permissionDecisionSource: 'user', dangerMatch: 'rm_recursive_or_force' }, 'user_approved_danger'],
    [{ permissionDecision: 'deny', permissionDecisionSource: 'sandbox_unavailable' }, 'sandbox_unavailable'],
    [{ permissionDecision: 'deny', permissionDecisionSource: 'builtin' }, 'rule_denied'],
    [{ permissionDecision: 'deny', permissionDecisionSource: 'non_interactive' }, 'rule_denied'],
    [{ permissionDecision: 'deny', permissionDecisionSource: 'user' }, 'user_denied'],
    [{ permissionDecision: 'allow', permissionDecisionSource: 'control_tool' }, 'control_tool'],
    [{ permissionDecision: 'cancelled', permissionDecisionSource: 'system' }, 'cancelled'],
    [{}, 'unknown'],
    [{ permissionDecision: 'allow', permissionDecisionSource: 'retired_source' }, 'unknown'],
  ])('classifies %j as %s', (attributes, expected) => {
    expect(permissionCategory(toolSpan(attributes))).toBe(expected)
  })

  it('reports a kernel denial even when the user approved the escalation', () => {
    expect(permissionCategory(toolSpan({
      permissionDecision: 'allow',
      permissionDecisionSource: 'user',
      escalationPaths: [{ path: '/w/.git', access: 'write', scope: 'subtree' }],
      sandboxDenied: true,
    }))).toBe('sandbox_denied')
  })

  it('does not classify model calls', () => {
    expect(permissionCategory({ ...toolSpan({}), kind: 'model_call' })).toBeNull()
  })
})
