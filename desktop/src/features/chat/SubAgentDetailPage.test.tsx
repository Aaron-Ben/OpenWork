import { renderToStaticMarkup } from 'react-dom/server'
import { afterEach, describe, expect, it, vi } from 'vitest'

import { coreCommands } from '@/bridge/commands'
import type { RuntimeLoadedSession, RuntimeSubAgentSessionRecord } from '@/bridge/compat'
import type { AgentRailItem } from './agentRailModel'
import { loadSubAgentTranscript, SubAgentDetailPage } from './SubAgentDetailPage'

const child: RuntimeSubAgentSessionRecord = {
  id: 'child-1',
  title: null,
  workingDirectory: '/repo',
  defaultModelId: 'model-1',
  status: 'active',
  createdAt: '2026-08-08T12:00:00+08:00',
  updatedAt: '2026-08-08T12:00:05+08:00',
  lastTurnAt: '2026-08-08T12:00:05+08:00',
  parentSessionId: 'parent-1',
  taskName: 'inspect_auth',
  agentRole: 'explorer',
  spawnSpanId: null,
}

const detail: RuntimeLoadedSession = {
  session: child,
  messages: [
    {
      id: 'child-user',
      turnId: 'child-turn',
      sequence: 1,
      role: 'user',
      content: [{ type: 'text', text: 'Inspect authentication.' }],
      messageKind: 'normal',
      createdAt: '2026-08-08T12:00:00+08:00',
    },
    {
      id: 'child-assistant',
      turnId: 'child-turn',
      sequence: 2,
      role: 'assistant',
      content: [{ type: 'text', text: 'Authentication is handled in auth.rs.' }],
      messageKind: 'normal',
      createdAt: '2026-08-08T12:00:05+08:00',
    },
  ],
  plans: [],
}

const item: AgentRailItem = {
  sessionId: 'child-1',
  role: 'explorer',
  task: 'inspect_auth',
  status: 'completed',
  toolActivity: null,
  elapsedMs: 2_000,
  tokens: 31_600,
  steps: 2,
  orchestrator: false,
}

function render(overrides: Partial<Parameters<typeof SubAgentDetailPage>[0]> = {}) {
  return renderToStaticMarkup(
    <SubAgentDetailPage
      item={item}
      sessionTitle="Q3 渠道 ROI 异常复盘"
      sidebarExpanded
      onToggleSidebar={vi.fn()}
      onBack={vi.fn()}
      {...overrides}
    />,
  )
}

afterEach(() => vi.restoreAllMocks())

describe('SubAgentDetailPage', () => {
  it('heads the page with the agent, its task, status, duration, and tokens', () => {
    const markup = render()

    expect(markup).toContain('data-sub-agent-header="true"')
    expect(markup).toContain('返回主控对话')
    expect(markup).toContain('Q3 渠道 ROI 异常复盘 / 子智能体')
    expect(markup).toContain('explorer')
    expect(markup).toContain('inspect_auth')
    expect(markup).toContain('已完成')
    expect(markup).toContain('00:02 · 31.6k')
  })

  it('is read only: no composer, no re-run, no direct dispatch', () => {
    const markup = render()

    expect(markup).not.toContain('textarea')
    expect(markup).not.toContain('重跑')
    expect(markup).not.toContain('直接下发')
    expect(markup).not.toContain('type="submit"')
  })

  it('shows a loading status until the persisted transcript arrives', () => {
    const markup = render()

    expect(markup).toContain('role="status"')
    expect(markup).toContain('正在读取只读对话记录')
  })

  it('offers a way back to the sidebar when it is collapsed', () => {
    // 这一页整个占掉中栏，收起侧栏后它就是最靠左的东西：让位与展开入口都得自己带。
    // 让位的像素数与平台有关，单独在 SidebarReveal 里测。
    expect(render({ sidebarExpanded: false })).toContain('aria-label="展开侧栏"')
    expect(render()).not.toContain('aria-label="展开侧栏"')
  })

  it('loads the transcript through the existing runtime_session_load bridge', async () => {
    vi.spyOn(coreCommands, 'loadSession').mockResolvedValue(detail)

    await expect(loadSubAgentTranscript('child-1')).resolves.toBe(detail)
    expect(coreCommands.loadSession).toHaveBeenCalledWith('child-1')
  })
})
