import { renderToStaticMarkup } from 'react-dom/server'
import { describe, expect, it, vi } from 'vitest'

import type { RuntimeTraceSummary } from '../../type/runtime'
import { filterRuntimeTraces, TraceSettingsList } from './TraceSettings'

const summaries: RuntimeTraceSummary[] = [
  {
    turnId: 'turn-1',
    sessionId: 'session-1',
    turnSequence: 1,
    status: 'failed',
    resolvedModelName: 'deepseek-v4-flash',
    modelCallCount: 2,
    toolCallCount: 1,
    spanCount: 3,
    startedAt: '2026-07-18T00:00:00Z',
    endedAt: '2026-07-18T00:00:01Z',
  },
  {
    turnId: 'turn-2',
    sessionId: 'session-2',
    turnSequence: 1,
    status: 'completed',
    resolvedModelName: 'glm-5.2',
    modelCallCount: 1,
    toolCallCount: 0,
    spanCount: 1,
    startedAt: '2026-07-18T00:01:00Z',
    endedAt: '2026-07-18T00:01:01Z',
  },
]

describe('TraceSettings', () => {
  it('filters V2 turn summaries by status and stable identifiers', () => {
    expect(filterRuntimeTraces(summaries, 'deepseek', 'failed')).toEqual([summaries[0]])
    expect(filterRuntimeTraces(summaries, 'session-2', 'all')).toEqual([summaries[1]])
  })

  it('renders model, session, model/tool counts and status', () => {
    const markup = renderToStaticMarkup(
      <TraceSettingsList summaries={summaries} onOpen={vi.fn()} />,
    )
    expect(markup).toContain('data-trace-row="turn-1"')
    expect(markup).toContain('deepseek-v4-flash')
    expect(markup).toContain('session-1')
    expect(markup).toContain('models 2')
    expect(markup).toContain('tools 1')
    expect(markup).toContain('bg-status-danger')
    expect(markup).toContain('bg-status-success')
  })
})
