import { renderToStaticMarkup } from 'react-dom/server'
import { describe, expect, it, vi } from 'vitest'

import type { TraceListItem } from '../traceViewModel'
import { TraceList } from './TraceList'

const item: TraceListItem = {
  traceId: 'turn-1',
  turnId: 'turn-1',
  sessionId: 'session-1',
  turnSequence: 2,
  status: 'completed',
  resolvedModelName: 'deepseek-v4-flash',
  modelCallCount: 1,
  modelSubmissionCount: 2,
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
    expect(markup).toContain('2026-07-18 08:00:00 (Asia/Shanghai)')
  })

  it('shows a turnless compaction trace instead of hiding it', () => {
    const compaction: TraceListItem = {
      ...item,
      traceId: 'trace-manual',
      turnId: null,
      turnSequence: null,
      modelCallCount: 0,
      modelSubmissionCount: 0,
      toolCallCount: 0,
      spanCount: 1,
      title: '手动压缩会话',
    }
    const markup = renderToStaticMarkup(
      <TraceList items={[compaction]} loading={false} onOpen={vi.fn()} />,
    )

    expect(markup).toContain('data-trace-row="trace-manual"')
    expect(markup).toContain('Conversation 压缩')
    // 没有 Turn 就没有调用计数可言，不能显示成两个 0
    expect(markup).not.toContain('0 次模型调用')
    expect(markup).not.toContain('0 次工具调用')
    // 无 Turn Trace 通过 trace_id 打开同一个详情抽屉，不能禁用。
    expect(markup).not.toContain('disabled')
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
