import { describe, expect, it } from 'vitest'

import type { CollabAgent, CollabAgentActivity, CollabRuntimeStatus } from '@/bridge/collab'
import { agentsInTab, engineTag, statusCounts } from './agentPageModel'

function agent(id: string, activity: CollabAgentActivity, archivedAt: string | null = null): CollabAgent {
  return {
    id, displayName: id, role: null, persona: '', engineId: 'opencode',
    mainModelId: 'm', triageModelId: 't', configRevision: 1, agendaEnabled: false, archivedAt, activity,
  }
}

const idle: CollabAgentActivity = { kind: 'idle', roomId: null, roomTitle: null, lastSpokeAt: null }
const agents = [
  agent('ada', { kind: 'working', roomId: null, roomTitle: null, cardId: null, cardTitle: null, startedAt: '2026-09-25T10:00:00+08:00' }),
  agent('bo', { kind: 'queued', cardCount: 1, firstCardTitle: 'Fix' }),
  agent('cy', idle),
  agent('dee', { kind: 'error', message: 'Engine missing' }),
  agent('eve', { kind: 'archived' }, '2026-09-20T10:00:00+08:00'),
]

function runtime(status: 'unknown' | 'ready' | 'missing' | 'error', lastError: string | null = null): CollabRuntimeStatus {
  return {
    runtimeSessionId: 'rt', startedAt: 0, lastComputerHeartbeat: 1, engineReadiness: [], runners: [],
    engines: [{ engineId: 'opencode', status, version: status === 'ready' ? '1.18.18' : null, checkedAt: 0, lastError, observedSessionId: 'rt' }],
  }
}

describe('agentsInTab', () => {
  it('splits active and archived Agents', () => {
    expect(agentsInTab(agents, 'active').map((item) => item.id)).toEqual(['ada', 'bo', 'cy', 'dee'])
    expect(agentsInTab(agents, 'archived').map((item) => item.id)).toEqual(['eve'])
  })
})

describe('statusCounts', () => {
  it('counts active Agents by what they are doing', () => {
    expect(statusCounts(agents)).toEqual({ working: 1, queued: 1, idle: 1, error: 1 })
  })
})

describe('engineTag', () => {
  it('shows the version with the sandbox when OpenCode is ready', () => {
    expect(engineTag(runtime('ready'))).toEqual({ tone: 'success', key: 'collab.agents.engine.ready', values: { version: '1.18.18' } })
  })

  it('shows the reason when OpenCode is missing or failed its check', () => {
    expect(engineTag(runtime('missing'))).toEqual({ tone: 'danger', key: 'collab.agents.engine.missing', values: {} })
    expect(engineTag(runtime('error', 'sandbox self-test failed'))).toEqual({
      tone: 'danger', key: 'collab.agents.engine.error', values: { reason: 'sandbox self-test failed' },
    })
  })

  it('shows a checking state before the first inventory', () => {
    expect(engineTag(null)).toEqual({ tone: 'neutral', key: 'collab.agents.engine.checking', values: {} })
    expect(engineTag(runtime('unknown'))).toEqual({ tone: 'neutral', key: 'collab.agents.engine.checking', values: {} })
  })
})
