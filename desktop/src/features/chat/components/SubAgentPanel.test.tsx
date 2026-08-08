import { renderToStaticMarkup } from 'react-dom/server'
import { afterEach, describe, expect, it, vi } from 'vitest'

import { coreCommands } from '@/bridge/commands'
import type { RuntimeLoadedSession, RuntimeSubAgentSessionRecord } from '@/bridge/compat'
import { createSessionRuntimeView } from '../runtimeReducer'
import { loadSubAgentTranscript, SubAgentPanelRow } from './SubAgentPanel'

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

describe('SubAgentPanel', () => {
  afterEach(() => vi.restoreAllMocks())

  it('renders child status, final summary, duration, and the full persistent transcript', () => {
    const markup = renderToStaticMarkup(
      <SubAgentPanelRow
        child={child}
        runtime={{
          ...createSessionRuntimeView(),
          turnId: 'child-turn',
          terminal: { status: 'completed', finalText: 'Authentication is in auth.rs.' },
          startedAtMs: 1_000,
          endedAtMs: 3_000,
        }}
        expanded
        detail={{ state: 'loaded', session: detail }}
        onToggle={vi.fn()}
      />,
    )

    expect(markup).toContain('inspect_auth')
    expect(markup).toContain('explorer')
    expect(markup).toContain('Authentication is in auth.rs.')
    expect(markup).toContain('2.00 s')
    expect(markup).toContain('Inspect authentication.')
    expect(markup).toContain('Authentication is handled in auth.rs.')
    expect(markup).toContain('data-readonly-sub-agent-transcript="true"')
    expect(markup).not.toContain('textarea')
  })

  it('loads expanded detail through the existing runtime_session_load bridge', async () => {
    vi.spyOn(coreCommands, 'loadSession').mockResolvedValue(detail)

    await expect(loadSubAgentTranscript('child-1')).resolves.toBe(detail)
    expect(coreCommands.loadSession).toHaveBeenCalledWith('child-1')
  })
})
