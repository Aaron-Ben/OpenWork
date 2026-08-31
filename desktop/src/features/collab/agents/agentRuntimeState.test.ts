import { describe, expect, it } from 'vitest'

import type { CollabAgent, CollabRuntimeStatus } from '@/bridge/collab'
import { agentActualState } from './agentRuntimeState'

const agent: CollabAgent = {
  id: 'agent-a',
  displayName: 'Agent A',
  role: null,
  persona: 'Test agent',
  engineId: 'opencode',
  mainModelId: 'deepseek-v4-flash',
  triageModelId: 'deepseek-v4-flash',
  configRevision: 3,
  agendaEnabled: true,
  archivedAt: null,
}

function runtime(
  overrides: Partial<Pick<CollabRuntimeStatus, 'engineReadiness' | 'runners'>> = {},
): CollabRuntimeStatus {
  return {
    runtimeSessionId: 'runtime-a',
    startedAt: 1,
    lastComputerHeartbeat: 1,
    engines: [],
    engineReadiness: [{ engineId: 'opencode', status: 'ready' }],
    runners: [],
    ...overrides,
  }
}

describe('agentActualState', () => {
  it('reports starting before the runner is available', () => {
    expect(agentActualState(runtime(), agent)).toEqual({ kind: 'starting' })
  })

  it('reports restarting while a runner still uses the old config revision', () => {
    expect(agentActualState(runtime({
      runners: [{ agentId: agent.id, configRevision: 2, state: 'running', lastError: null }],
    }), agent)).toEqual({ kind: 'restarting' })
  })

  it('reports running once the runner has applied the current config', () => {
    expect(agentActualState(runtime({
      runners: [{ agentId: agent.id, configRevision: 3, state: 'running', lastError: null }],
    }), agent)).toEqual({ kind: 'running' })
  })

  it('keeps runner errors more specific than engine readiness', () => {
    expect(agentActualState(runtime({
      engineReadiness: [{ engineId: 'opencode', status: 'missing' }],
      runners: [{ agentId: agent.id, configRevision: 3, state: 'error', lastError: 'crashed' }],
    }), agent)).toEqual({ kind: 'error', detail: 'crashed' })
  })

  it('reports a missing engine when no runner exists', () => {
    expect(agentActualState(runtime({
      engineReadiness: [{ engineId: 'opencode', status: 'missing' }],
    }), agent)).toEqual({ kind: 'engineMissing' })
  })
})
