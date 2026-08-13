import { describe, expect, it } from 'vitest'

import type { RuntimeTraceSpan, RuntimeTurnTrace } from '@/bridge/compat'
import { contextUsageFromTrace } from './contextUsage'

function modelSpan(
  order: number,
  inputTokens: number | null,
  estimatedInputTokens?: number,
): RuntimeTraceSpan {
  return {
    id: `model-${order}`,
    traceId: 'turn-1',
    sessionId: 'session-1',
    turnId: 'turn-1',
    parentSpanId: null,
    kind: 'model_call',
    name: 'model',
    status: 'succeeded',
    modelId: 'model-1',
    resolvedModelName: 'deepseek-v4-flash',
    providerRequestId: null,
    providerCallId: null,
    requestedToolName: null,
    resolvedToolName: null,
    attemptCount: 1,
    inputTokens,
    outputTokens: null,
    cachedInputTokens: null,
    reasoningTokens: null,
    totalTokens: null,
    responseMessageId: null,
    permissionWaitMs: null,
    startedAt: `2026-07-22T00:00:0${order}Z`,
    endedAt: `2026-07-22T00:00:0${order + 1}Z`,
    errorCode: null,
    errorMessage: null,
    attributes: estimatedInputTokens === undefined
      ? {}
      : { requestEstimatedInputTokens: estimatedInputTokens },
  }
}

function trace(spans: RuntimeTraceSpan[]): RuntimeTurnTrace {
  return {
    summary: {
      traceId: 'turn-1',
      turnId: 'turn-1',
      sessionId: 'session-1',
      turnSequence: 1,
      status: 'completed',
      resolvedModelName: 'deepseek-v4-flash',
      modelCallCount: spans.filter((span) => span.kind === 'model_call').length,
      modelSubmissionCount: spans.filter((span) => span.kind === 'model_call').length,
      toolCallCount: spans.filter((span) => span.kind === 'tool_call').length,
      spanCount: spans.length,
      totalTokens: 0,
      startedAt: '2026-07-22T00:00:00Z',
      endedAt: '2026-07-22T00:00:01Z',
    },
    spans,
    completeness: {
      expectedModelCalls: 1,
      capturedModelCalls: 1,
      expectedToolCalls: 0,
      capturedToolCalls: 0,
      orphanToolSpans: 0,
      runningSpans: 0,
      outcomeUnknownSpans: 0,
      state: 'complete',
    },
  }
}

describe('contextUsageFromTrace', () => {
  it('uses provider input tokens from the latest model call', () => {
    const usage = contextUsageFromTrace(
      trace([modelSpan(1, 12_000), modelSpan(3, 66_000, 64_000)]),
      200_000,
    )

    expect(usage).toEqual({
      usedTokens: 66_000,
      totalTokens: 200_000,
      estimated: false,
    })
  })

  it('falls back to the preflight estimate while provider usage is unavailable', () => {
    const usage = contextUsageFromTrace(trace([modelSpan(1, null, 4_200)]), 200_000)

    expect(usage).toEqual({
      usedTokens: 4_200,
      totalTokens: 200_000,
      estimated: true,
    })
  })

  it('returns no usage without a valid capacity or token count', () => {
    expect(contextUsageFromTrace(trace([modelSpan(1, null)]), 200_000)).toBeNull()
    expect(contextUsageFromTrace(trace([modelSpan(1, 10)]), 0)).toBeNull()
  })
})
