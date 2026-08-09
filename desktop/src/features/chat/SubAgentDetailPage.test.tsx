import { renderToStaticMarkup } from 'react-dom/server'
import { afterEach, describe, expect, it, vi } from 'vitest'

import { coreCommands } from '@/bridge/commands'
import type { RuntimeLoadedSession, RuntimeSubAgentSessionRecord } from '@/bridge/compat'
import type { AgentRailItem } from './agentRailModel'
import {
  loadSubAgentTranscript,
  ReadonlySubAgentTranscript,
  SubAgentDetailPage,
} from './SubAgentDetailPage'

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
  it('keeps the detail header focused on navigation instead of repeating the rail summary', () => {
    const markup = render()

    expect(markup).toContain('data-sub-agent-header="true"')
    expect(markup).toContain('返回主控对话')
    expect(markup).toContain('Q3 渠道 ROI 异常复盘 / 子智能体')
    expect(markup).not.toContain('explorer')
    expect(markup).not.toContain('inspect_auth')
    expect(markup).not.toContain('已完成')
    expect(markup).not.toContain('00:02 · 31.6k')
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

const toolTurn: RuntimeLoadedSession['messages'] = [
  {
    id: 'child-call',
    turnId: 'child-turn',
    sequence: 1,
    role: 'assistant',
    content: [{
      type: 'tool_call',
      id: 'provider-read',
      name: 'read',
      input: JSON.stringify({ path: '/repo/SKILL.md' }),
      state: 'submitted',
    }],
    messageKind: 'normal',
    createdAt: '2026-08-09T22:00:02+08:00',
  },
  {
    id: 'child-result',
    turnId: 'child-turn',
    sequence: 2,
    role: 'tool',
    content: [{
      type: 'tool_result',
      id: 'provider-read',
      name: 'read',
      output: [{ type: 'text', text: '     1\tfirst\n     2\tsecond' }],
      state: 'success',
    }],
    messageKind: 'normal',
    createdAt: '2026-08-09T22:00:03+08:00',
  },
]

describe('ReadonlySubAgentTranscript', () => {
  it('pairs a persisted call with its result instead of leaving the call spinning', () => {
    const markup = renderToStaticMarkup(
      <ReadonlySubAgentTranscript messages={toolTurn} workspaceRoot="/repo" />,
    )

    expect(markup).not.toContain('animate-spin')
    expect(markup).not.toContain('aria-label="执行中"')
    // 一次调用一行：配不上对时会裂成"只有路径"和"只有行数"两行。
    expect(markup.split('data-tool-activity-row=')).toHaveLength(2)
    // 路径来自 tool_call、行数来自 tool_result，两者都落在这一行里。
    expect(markup).toContain('/repo/SKILL.md')
    expect(markup).toContain('2 行')
  })
})
