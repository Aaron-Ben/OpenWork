import { renderToStaticMarkup } from 'react-dom/server'
import { describe, expect, it, vi } from 'vitest'

import type { TurnTraceSummary } from '../../type/trace'
import { buildTraceListQuery, TraceSettingsList } from './TraceSettings'

const summaries: TurnTraceSummary[] = [
  {
    traceId: 'turn-1', turnId: 'turn-1', sessionId: 'session-1', status: 'failed',
    model: 'glm-5.1', startedAt: 1_000, endedAt: 2_000, durationMs: 1_000,
    stepCount: 2, modelAttemptCount: 2, transportAttemptCount: 3, toolRunCount: 1,
    approvalCount: 0, retryCount: 1, inputTokens: 100, outputTokens: 20,
    errorCount: 1, recovered: false, sessionTitle: '修复持久化',
    workingDir: '/Volumes/Code/OpenWork', inputPreview: '请修复 trace 持久化',
    diagnosis: { status: 'blocked', reason: 'model_error', focusSpanId: 'model-1', evidenceSpanIds: ['model-1'] },
    dataCompleteness: 'complete',
  },
  {
    traceId: 'turn-2', turnId: 'turn-2', sessionId: 'missing', status: 'succeeded',
    model: 'deepseek-chat', startedAt: 3_000, endedAt: 4_000, durationMs: 1_000,
    stepCount: 1, modelAttemptCount: 1, transportAttemptCount: 1, toolRunCount: 0,
    approvalCount: 0, retryCount: 0, inputTokens: 50, outputTokens: 10,
    errorCount: 0, recovered: false, sessionTitle: null, workingDir: null,
    inputPreview: null,
    diagnosis: { status: 'healthy', reason: 'healthy', focusSpanId: 'turn-2', evidenceSpanIds: ['turn-2'] },
    dataCompleteness: 'legacy',
  },
]

describe('TraceSettings', () => {
  it('builds the server-side query instead of filtering the loaded page', () => {
    expect(buildTraceListQuery(' cargo test ', 'failed', 50)).toEqual({
      limit: 50,
      offset: 50,
      query: 'cargo test',
      status: 'failed',
    })
  })

  it('renders a global trace row with context and an accessible detail action', () => {
    const markup = renderToStaticMarkup(
      <TraceSettingsList
        summaries={summaries}
        onOpen={vi.fn()}
      />,
    )

    expect(markup).toContain('data-settings-trace-list="true"')
    expect(markup).toContain('data-trace-row="turn-1"')
    expect(markup).toContain('修复持久化')
    expect(markup).toContain('OpenWork')
    expect(markup).toContain('请修复 trace 持久化')
    expect(markup).toContain('glm-5.1')
    expect(markup).toContain('查看运行详情')
    expect(markup).toContain('bg-status-danger')
    expect(markup).toContain('bg-status-success')
    expect(markup).toContain('bg-status-warning-soft')
    expect(markup).toContain('text-status-warning-ink')
    expect(markup).not.toContain('bg-amber-50')
    expect(markup).not.toContain('bg-emerald-500')
  })
})
