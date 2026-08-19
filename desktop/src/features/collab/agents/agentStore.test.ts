import { beforeEach, describe, expect, it, vi } from 'vitest'

import { collabCommands, type CollabAgentInput } from '@/bridge/collab'
import { useAgentStore } from './agentStore'

vi.mock('@/bridge/collab', () => ({
  collabCommands: { listAgents: vi.fn(), createAgent: vi.fn(), updateAgent: vi.fn() },
}))

const input: CollabAgentInput = {
  id: 'alice', displayName: 'Alice', role: null, bio: null,
  systemPrompt: 'Be clear', providerId: 'provider', modelId: 'model', enabled: true,
}

describe('agentStore', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    useAgentStore.setState({ agents: [], loading: false, error: null })
  })

  it('keeps disabled definitions in canonical history while updating participation', async () => {
    vi.mocked(collabCommands.updateAgent).mockResolvedValue({ ...input, opencodeSessionId: null, enabled: false })
    vi.mocked(collabCommands.listAgents).mockResolvedValue([{ ...input, opencodeSessionId: null, enabled: false }])
    await useAgentStore.getState().update({ ...input, enabled: false })
    expect(useAgentStore.getState().agents).toMatchObject([{ id: 'alice', enabled: false }])
  })
})
