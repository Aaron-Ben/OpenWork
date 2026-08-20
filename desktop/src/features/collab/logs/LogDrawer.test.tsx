import { renderToStaticMarkup } from 'react-dom/server'
import { describe, expect, it } from 'vitest'

import type { CollabLogEntry } from '@/bridge/collab'
import { LogTimeline, logSummary } from './LogDrawer'

const entries: CollabLogEntry[] = [
  {
    source: 'event', id: 'evt_2', runId: 'run_1', agentId: 'alice', roomId: 'general',
    kind: 'prompt.completed', payload: { status: 'completed', outcome: 'unpublished' },
    createdAt: '2026-08-19T10:00:02+08:00',
  },
  {
    source: 'triage', id: 'triage_1', runId: null, agentId: 'alice', roomId: 'general',
    kind: 'triage.decision', payload: { actionable: true, reason: 'owns card' },
    createdAt: '2026-08-19T10:00:00+08:00',
  },
]

describe('LogTimeline', () => {
  it('renders the three-table feed as a flat ordered list with Beijing time', () => {
    const html = renderToStaticMarkup(<LogTimeline entries={entries} />)
    expect(html).toContain('data-log-source="event"')
    expect(html).toContain('data-log-source="triage"')
    expect(html).toContain('data-run-outcome="unpublished"')
    expect(html).toContain('border-red-500 bg-red-50')
    expect(html.indexOf('prompt.completed')).toBeLessThan(html.indexOf('triage.decision'))
    expect(html).toContain('2026-08-19 10:00:02 (Asia/Shanghai)')
    expect(html).not.toContain('data-span')
  })

  it('summarizes decisions and usage through localized labels', () => {
    const labels = {
      actionable: '需要处理',
      notActionable: '无需处理',
      tokenSummary: (input: number, cached: number, output: number) =>
        `输入 ${input}，缓存 ${cached}，输出 ${output}`,
    }
    expect(logSummary({ actionable: false }, labels)).toBe('无需处理')
    expect(logSummary({ inputTokens: 7, cachedInputTokens: 2, outputTokens: 3 }, labels))
      .toBe('输入 7，缓存 2，输出 3')
  })
})
