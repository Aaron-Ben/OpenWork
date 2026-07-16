import { renderToStaticMarkup } from 'react-dom/server'
import { describe, expect, it } from 'vitest'

import type { TraceSpan, TurnTraceSummary } from '../../type/trace'
import { TurnTraceSummaryButton } from './TurnTraceSummaryButton'
import {
  buildTraceTree,
  buildWaterfallTicks,
  calculateWaterfallSegment,
  filterVisibleTraceRows,
  flattenTraceTree,
  formatTraceSummary,
} from './traceViewModel'

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
  sessionTitle: 'Trace upgrade',
  workingDir: '/Volumes/Code/OpenWork',
  inputPreview: 'upgrade trace',
  diagnosis: {
    status: 'healthy', reason: 'healthy', focusSpanId: 'turn-1', evidenceSpanIds: ['turn-1'],
  },
  dataCompleteness: 'complete',
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
    expect(flattenTraceTree(tree).map((node) => [node.span.spanId, node.depth])).toEqual([
      ['turn-1', 0],
      ['step-1', 1],
      ['tool-1', 2],
    ])
  })

  it('positions each span against the same turn clock for a proportional waterfall', () => {
    const segment = calculateWaterfallSegment(spans[2], summary.startedAt, summary.durationMs)

    expect(segment.leftPercent).toBeCloseTo(5.38, 1)
    expect(segment.widthPercent).toBeCloseTo(10.75, 1)
  })

  it('builds LangSmith-style nice ticks and increases detail when zoomed', () => {
    const fitted = buildWaterfallTicks(14_260, 1)
    const zoomed = buildWaterfallTicks(14_260, 2)

    expect(fitted.slice(0, 4).map((tick) => tick.valueMs)).toEqual([0, 1_000, 2_000, 3_000])
    expect(fitted[fitted.length - 1]?.valueMs).toBe(14_000)
    expect(fitted.every((tick) => tick.leftPercent >= 0 && tick.leftPercent <= 100)).toBe(true)
    expect(zoomed.length).toBeGreaterThan(fitted.length)
  })

  it('hides every descendant of a collapsed tree node', () => {
    const rows = flattenTraceTree(buildTraceTree(spans))

    expect(filterVisibleTraceRows(rows, new Set(['step-1'])).map((node) => node.span.spanId)).toEqual([
      'turn-1',
      'step-1',
    ])
    expect(filterVisibleTraceRows(rows, new Set(['turn-1'])).map((node) => node.span.spanId)).toEqual([
      'turn-1',
    ])
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
