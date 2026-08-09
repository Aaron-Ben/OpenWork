import { renderToStaticMarkup } from 'react-dom/server'
import { describe, expect, it, vi } from 'vitest'

import type { RuntimeSessionRecord } from '@/bridge/compat'
import { SessionItem } from './SessionItem'

function session(overrides: Partial<RuntimeSessionRecord> = {}): RuntimeSessionRecord {
  return {
    id: 'session-1',
    title: 'Q3 渠道 ROI 异常复盘',
    workingDirectory: '/repo',
    defaultModelId: 'model-1',
    status: 'active',
    createdAt: '2026-08-08T09:00:00+08:00',
    updatedAt: '2026-08-08T14:02:00+08:00',
    lastTurnAt: '2026-08-08T14:02:00+08:00',
    parentSessionId: null,
    taskName: null,
    agentRole: null,
    spawnSpanId: null,
    ...overrides,
  }
}

function render(overrides: Partial<Parameters<typeof SessionItem>[0]> = {}) {
  return renderToStaticMarkup(
    <SessionItem
      session={session()}
      active={false}
      activity="idle"
      onSelect={vi.fn()}
      onRename={vi.fn()}
      onDelete={vi.fn()}
      {...overrides}
    />,
  )
}

describe('SessionItem', () => {
  it('is a single line: the title and nothing else', () => {
    const markup = render()

    expect(markup).toContain('data-session-row="session-1"')
    expect(markup).toContain('Q3 渠道 ROI 异常复盘')
    // 时间、终态、智能体数都不进侧栏。
    expect(markup).not.toContain('14:02')
    expect(markup).not.toContain('2026-08-08')
    expect(markup).not.toContain('已完成')
    expect(markup).not.toContain('智能体')
    expect(markup).not.toContain('border-l')
  })

  it('falls back to the placeholder title for an unnamed session', () => {
    expect(render({ session: session({ title: null }) })).toContain('新会话')
    expect(render({ session: session({ title: '   ' }) })).toContain('新会话')
  })

  it('marks the active session with a filled block, not an accent bar or accent text', () => {
    const markup = render({ active: true })

    expect(markup).toContain('aria-current="page"')
    expect(markup).toContain('bg-clay-soft')
    expect(markup).toContain('h-10')
    expect(markup).toContain('rounded-full')
    expect(markup).toContain('bg-clay')
    expect(markup).toContain('font-medium text-ink')
    expect(markup).not.toContain('text-clay')
  })

  it('shows a status dot only while the session is doing something', () => {
    const running = render({ activity: 'running_tools' })
    const waiting = render({ activity: 'waiting_permission' })
    const idle = render()

    expect(running).toContain('bg-clay')
    expect(running).toContain('title="运行中"')
    expect(waiting).toContain('bg-status-warning')
    expect(waiting).toContain('title="等待输入"')
    // 静止的会话不挂圆点，列表才不会看起来像一串项目符号。
    expect(idle).not.toContain('size-1.5 shrink-0 rounded-full')
  })

  it('keeps rename and delete out of the way until hover', () => {
    const markup = render()

    expect(markup).toContain('aria-label="重命名会话"')
    expect(markup).toContain('aria-label="删除会话"')
    expect(markup).toContain('opacity-0 transition group-hover:opacity-100')
  })
})
