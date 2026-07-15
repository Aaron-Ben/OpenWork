import { renderToStaticMarkup } from 'react-dom/server'
import { describe, expect, it, vi } from 'vitest'

import type { SessionSummary } from '../../type/session'
import type { TurnTraceSummary } from '../../type/trace'
import { filterTraceList, TraceSettingsList } from './TraceSettings'

const sessions: SessionSummary[] = [
  {
    id: 'session-1',
    title: '修复持久化',
    providerId: 'provider-1',
    model: 'glm-5.1',
    workingDir: '/Volumes/Code/OpenWork',
    updatedAt: 1,
  },
]

const summaries: TurnTraceSummary[] = [
  {
    traceId: 'turn-1', turnId: 'turn-1', sessionId: 'session-1', status: 'failed',
    model: 'glm-5.1', startedAt: 1_000, endedAt: 2_000, durationMs: 1_000,
    stepCount: 2, modelAttemptCount: 2, transportAttemptCount: 3, toolRunCount: 1,
    approvalCount: 0, retryCount: 1, inputTokens: 100, outputTokens: 20,
    errorCount: 1, recovered: false,
  },
  {
    traceId: 'turn-2', turnId: 'turn-2', sessionId: 'missing', status: 'succeeded',
    model: 'deepseek-chat', startedAt: 3_000, endedAt: 4_000, durationMs: 1_000,
    stepCount: 1, modelAttemptCount: 1, transportAttemptCount: 1, toolRunCount: 0,
    approvalCount: 0, retryCount: 0, inputTokens: 50, outputTokens: 10,
    errorCount: 0, recovered: false,
  },
]

describe('TraceSettings', () => {
  it('filters by project, conversation, model and status', () => {
    expect(filterTraceList(summaries, sessions, 'OpenWork', 'all')).toHaveLength(1)
    expect(filterTraceList(summaries, sessions, '修复持久化', 'all')).toHaveLength(1)
    expect(filterTraceList(summaries, sessions, 'deepseek', 'all')).toHaveLength(1)
    expect(filterTraceList(summaries, sessions, '', 'failed')).toEqual([summaries[0]])
  })

  it('renders a global trace row with context and an accessible detail action', () => {
    const markup = renderToStaticMarkup(
      <TraceSettingsList
        summaries={summaries}
        sessions={sessions}
        query=""
        status="all"
        onOpen={vi.fn()}
      />,
    )

    expect(markup).toContain('data-settings-trace-list="true"')
    expect(markup).toContain('data-trace-row="turn-1"')
    expect(markup).toContain('修复持久化')
    expect(markup).toContain('OpenWork')
    expect(markup).toContain('glm-5.1')
    expect(markup).toContain('查看运行详情')
  })
})
