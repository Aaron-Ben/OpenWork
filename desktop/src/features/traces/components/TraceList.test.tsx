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
  totalTokens: 1234,
  startedAt: '2026-07-18T00:00:00.000Z',
  endedAt: '2026-07-18T00:00:02.000Z',
  title: '修复登录流程',
  workingDirectory: '/repo/openwork',
  durationMs: 2000,
}

describe('TraceList', () => {
  it('renders a sortable table with recognizable session context', () => {
    const markup = renderToStaticMarkup(
      <TraceList items={[item]} loading={false} onOpen={vi.fn()} />,
    )

    expect(markup).toContain('修复登录流程')
    expect(markup).toContain('/repo/openwork')
    expect(markup).toContain('已完成')
    expect(markup).toContain('deepseek-v4-flash')
    expect(markup).toContain('2.00 s')
    expect(markup).toContain('2026-07-18 08:00:00 (Asia/Shanghai)')
    // 表格列头与可排序控件
    for (const header of ['状态', '运行', '模型', '模型调用', '工具调用', 'Token', '耗时', '开始时间']) {
      expect(markup).toContain(header)
    }
    for (const key of ['resolvedModelName', 'modelSubmissionCount', 'toolCallCount', 'totalTokens', 'durationMs', 'startedAt']) {
      expect(markup).toContain(`data-sort-key="${key}"`)
    }
    expect(markup).toContain('data-model-calls="2"')
    expect(markup).toContain('data-tool-calls="3"')
    expect(markup).toContain('data-total-tokens="1234"')
  })

  it('shows a turnless compaction trace with measured tokens but no call counts', () => {
    const compaction: TraceListItem = {
      ...item,
      traceId: 'trace-manual',
      turnId: null,
      turnSequence: null,
      modelCallCount: 0,
      modelSubmissionCount: 0,
      toolCallCount: 0,
      spanCount: 1,
      totalTokens: 42,
      resolvedModelName: '',
      title: '手动压缩会话',
    }
    const markup = renderToStaticMarkup(
      <TraceList items={[compaction]} loading={false} onOpen={vi.fn()} />,
    )

    expect(markup).toContain('data-trace-row="trace-manual"')
    expect(markup).toContain('Conversation 压缩')
    // 没有 Turn 就没有调用计数可言，不能以 0 充数
    expect(markup).not.toContain('data-model-calls')
    expect(markup).not.toContain('data-tool-calls')
    // token 是 span 实测合计，照常显示
    expect(markup).toContain('data-total-tokens="42"')
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
