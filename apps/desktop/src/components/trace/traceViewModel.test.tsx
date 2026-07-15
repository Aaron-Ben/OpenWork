import { renderToStaticMarkup } from 'react-dom/server'
import { describe, expect, it } from 'vitest'

import type { TraceSpan, TurnTraceSummary } from '../../type/trace'
import { TurnTraceSummaryButton } from './TurnTraceSummaryButton'
import { buildTraceTree, formatTraceSummary } from './traceViewModel'

const summary: TurnTraceSummary = {
  traceId: 'turn-1',
  turnId: 'turn-1',
  sessionId: 'session-1',
  status: 'succeeded',
  model: 'glm-5.1',
  startedAt: 1_000,
  endedAt: 19_600,
  durationMs: 18_600,
  stepCount: 3,
  modelAttemptCount: 3,
  transportAttemptCount: 4,
  toolRunCount: 2,
  approvalCount: 1,
  retryCount: 1,
  inputTokens: 6_400,
  outputTokens: 2_020,
  errorCount: 0,
  recovered: false,
}

const spans: TraceSpan[] = [
  {
    traceId: 'turn-1', spanId: 'turn-1', parentSpanId: null, spanKind: 'turn',
    spanName: 'turn.run', status: 'succeeded', sessionId: 'session-1', turnId: 'turn-1',
    stepId: null, toolRunId: null, startedAt: 1_000, endedAt: 19_600,
    durationMs: 18_600, attributes: {}, errorType: null, errorCode: null, errorMessage: null,
  },
  {
    traceId: 'turn-1', spanId: 'step-1', parentSpanId: 'turn-1', spanKind: 'step',
    spanName: 'step.run', status: 'succeeded', sessionId: 'session-1', turnId: 'turn-1',
    stepId: 'step-1', toolRunId: null, startedAt: 1_100, endedAt: 19_500,
    durationMs: 18_400, attributes: { stepIndex: 1 }, errorType: null, errorCode: null,
    errorMessage: null,
  },
  {
    traceId: 'turn-1', spanId: 'tool-1', parentSpanId: 'step-1', spanKind: 'tool_run',
    spanName: 'tool.run', status: 'succeeded', sessionId: 'session-1', turnId: 'turn-1',
    stepId: 'step-1', toolRunId: 'tool-1', startedAt: 2_000, endedAt: 4_000,
    durationMs: 2_000, attributes: { toolName: 'bash' }, errorType: null, errorCode: null,
    errorMessage: null,
  },
]

describe('Trace V1 view model', () => {
  it('builds the tree from stable parent ids instead of timestamps', () => {
    const tree = buildTraceTree([spans[2], spans[0], spans[1]])

    expect(tree).toHaveLength(1)
    expect(tree[0].span.spanId).toBe('turn-1')
    expect(tree[0].children[0].span.spanId).toBe('step-1')
    expect(tree[0].children[0].children[0].span.spanId).toBe('tool-1')
  })

  it('formats the compact user-facing summary without exposing raw payloads', () => {
    expect(formatTraceSummary(summary, 'zh-CN')).toBe(
      '运行 3 步 · 模型 3 次 · 工具 2 次 · 18.6 秒 · 8,420 Tokens · 重试 1 次',
    )
  })

  it('renders one accessible button that opens the trace detail', () => {
    const markup = renderToStaticMarkup(
      <TurnTraceSummaryButton summary={summary} onOpen={() => undefined} />,
    )

    expect(markup).toContain('data-turn-trace-summary="turn-1"')
    expect(markup).toContain('运行 3 步')
    expect(markup).toContain('8,420 Tokens')
    expect(markup).toContain('<button')
  })
})
