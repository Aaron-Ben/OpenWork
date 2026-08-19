import { beforeEach, describe, expect, it, vi } from 'vitest'

import { collabCommands, type CollabAgent, type CollabAgentInput } from '@/bridge/collab'
import { useAgentStore } from './agentStore'

vi.mock('@/bridge/collab', () => ({
  collabCommands: { listAgents: vi.fn(), createAgent: vi.fn(), updateAgent: vi.fn() },
}))

const input: CollabAgentInput = {
  id: 'alice', displayName: 'Alice', role: null, bio: null,
  systemPrompt: 'Be clear', providerId: 'provider', modelId: 'model', enabled: true,
  scannerEnabled: false,
}

function agent(overrides: Partial<CollabAgent> = {}): CollabAgent {
  return {
    ...input,
    id: 'alice',
    opencodeSessionId: null,
    activity: { kind: 'idle' },
    ...overrides,
  }
}

describe('agentStore', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    useAgentStore.setState({ agents: [], loading: false, error: null })
  })

  it('keeps disabled definitions in canonical history while updating participation', async () => {
    vi.mocked(collabCommands.updateAgent).mockResolvedValue(agent({ enabled: false }))
    vi.mocked(collabCommands.listAgents).mockResolvedValue([agent({ enabled: false })])
    await useAgentStore.getState().update({ ...input, enabled: false })
    expect(useAgentStore.getState().agents).toMatchObject([{ id: 'alice', enabled: false }])
  })

  it('applies daemon-normalized activity without knowing OpenCode event names', () => {
    useAgentStore.setState({ agents: [agent({ opencodeSessionId: 'ses_1' })] })
    useAgentStore.getState().applyActivity('alice', {
      kind: 'executing',
      detail: '$ cargo test',
    })
    expect(useAgentStore.getState().agents[0]?.activity).toEqual({
      kind: 'executing',
      detail: '$ cargo test',
    })
  })
})
