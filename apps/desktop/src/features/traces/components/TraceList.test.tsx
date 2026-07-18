import { renderToStaticMarkup } from 'react-dom/server'
import { describe, expect, it, vi } from 'vitest'

import type { TraceListItem } from '../traceViewModel'
import { TraceList } from './TraceList'

const item: TraceListItem = {
  turnId: 'turn-1',
  sessionId: 'session-1',
  turnSequence: 2,
  status: 'completed',
  resolvedModelName: 'deepseek-v4-flash',
  modelCallCount: 2,
  toolCallCount: 3,
  spanCount: 5,
  startedAt: '2026-07-18T00:00:00.000Z',
  endedAt: '2026-07-18T00:00:02.000Z',
  title: '修复登录流程',
  workingDirectory: '/repo/openwork',
  durationMs: 2000,
}

describe('TraceList', () => {
  it('renders recognizable session context and a textual status', () => {
    const markup = renderToStaticMarkup(
      <TraceList items={[item]} loading={false} onOpen={vi.fn()} />,
    )

    expect(markup).toContain('修复登录流程')
    expect(markup).toContain('/repo/openwork')
    expect(markup).toContain('已完成')
    expect(markup).toContain('2 次模型调用')
    expect(markup).toContain('3 次工具调用')
    expect(markup).toContain('2.00 s')
  })

  it('distinguishes loading from an empty result', () => {
    const loading = renderToStaticMarkup(
      <TraceList items={[]} loading onOpen={vi.fn()} />,
    )
    const empty = renderToStaticMarkup(
      <TraceList items={[]} loading={false} onOpen={vi.fn()} />,
    )

    expect(loading).toContain('data-trace-loading="true"')
    expect(loading).not.toContain('没有符合条件的运行记录')
    expect(empty).toContain('没有符合条件的运行记录')
  })
})
