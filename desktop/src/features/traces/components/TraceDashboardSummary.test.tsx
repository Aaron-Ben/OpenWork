import { renderToStaticMarkup } from 'react-dom/server'
import { describe, expect, it } from 'vitest'

import type { TraceListItem } from '../traceViewModel'
import { TraceDashboardSummary } from './TraceDashboardSummary'

const base: TraceListItem = {
  traceId: 'trace-1', turnId: 'turn-1', sessionId: 'session-1', turnSequence: 1,
  status: 'completed', resolvedModelName: 'deepseek-v4-flash', modelCallCount: 2,
  modelSubmissionCount: 2, toolCallCount: 3, spanCount: 5, totalTokens: 1_000_000,
  startedAt: '2026-08-09T01:00:00.000Z', endedAt: '2026-08-09T01:01:30.000Z',
  title: '新会话', workingDirectory: '/repo/openwork', durationMs: 90_000,
}

describe('TraceDashboardSummary', () => {
  it('summarizes only today runs with success rate, median duration, and tokens', () => {
    const items = [
      base,
      {
        ...base,
        traceId: 'trace-2',
        status: 'failed',
        totalTokens: 2_000_000,
        durationMs: 120_000,
        startedAt: '2026-08-09T02:00:00.000Z',
      },
      {
        ...base,
        traceId: 'trace-yesterday',
        totalTokens: 9_000_000,
        startedAt: '2026-08-08T01:00:00.000Z',
      },
      {
        ...base,
        traceId: 'trace-running',
        status: 'running',
        totalTokens: 4_000_000,
        durationMs: 999_000,
        endedAt: null,
      },
      {
        ...base,
        traceId: 'trace-compaction',
        turnId: null,
        totalTokens: 8_000_000,
        durationMs: 888_000,
      },
    ]
    const markup = renderToStaticMarkup(
      <TraceDashboardSummary items={items} now={Date.parse('2026-08-09T12:00:00.000Z')} />,
    )

    expect(markup).toContain('data-trace-dashboard-summary="true"')
    expect(markup).not.toContain('基于当前已加载记录')
    expect(markup).toContain('今日运行')
    expect(markup).toMatch(/data-dashboard-value="runCount"[^>]*>3<\/strong>/)
    expect(markup).toContain('成功率')
    expect(markup).toContain('50%')
    expect(markup).toContain('1 次失败')
    expect(markup).toContain('中位耗时')
    expect(markup).toContain('105 s')
    expect(markup).toContain('Token 合计')
    expect(markup).toContain('7.0 M')
  })
})
