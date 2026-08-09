import { renderToStaticMarkup } from 'react-dom/server'
import { describe, expect, it, vi } from 'vitest'

import i18n from '@/i18n'
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
  it('renders date-grouped run cards with context, scale bars, and relative time', () => {
    const markup = renderToStaticMarkup(
      <TraceList
        items={[item]}
        loading={false}
        now={Date.parse('2026-07-18T02:00:00.000Z')}
        onOpen={vi.fn()}
      />,
    )

    expect(markup).toContain('data-trace-date-group="2026-07-18"')
    expect(markup).toContain('今天 · 7月18日')
    expect(markup).toContain('data-trace-run-card="turn-1"')
    expect(markup).toContain('修复登录流程')
    expect(markup).toContain('/repo/openwork')
    expect(markup).toContain('已完成')
    expect(markup).toContain('deepseek-v4-flash')
    expect(markup).toContain('2.00 s')
    expect(markup).toContain('2 模型')
    expect(markup).toContain('3 工具')
    expect(markup).toContain('2 小时前')
    expect(markup).toContain('08:00:00')
    expect(markup).toContain('data-duration-percent="100"')
    expect(markup).toContain('data-token-percent="100"')
    expect(markup).toContain('max-[820px]:grid-cols-[minmax(0,1fr)_96px]')
    expect(markup).not.toContain('data-sort-key=')
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
    expect(markup).toContain('data-trace-run-card="trace-manual"')
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

  it('uses singular English relative-time labels', async () => {
    await i18n.changeLanguage('en-US')
    try {
      const markup = renderToStaticMarkup(
        <TraceList
          items={[item]}
          loading={false}
          now={Date.parse('2026-07-19T00:00:00.000Z')}
          onOpen={vi.fn()}
        />,
      )
      expect(markup).toContain('1 day ago')
      expect(markup).not.toContain('1 days ago')
    } finally {
      await i18n.changeLanguage('zh-CN')
    }
  })
})
