import { beforeEach, describe, expect, it, vi } from 'vitest'

import { collabCommands, type CollabRuntimeStatus } from '@/bridge/collab'
import { useCollabRuntimeStore } from './runtimeStore'

vi.mock('@/bridge/collab', () => ({
  collabCommands: { status: vi.fn() },
}))

const status: CollabRuntimeStatus = {
  runtimeSessionId: 'runtime-test',
  startedAt: 1,
  lastComputerHeartbeat: 2,
  engines: [],
  engineReadiness: [{ engineId: 'opencode', status: 'ready' }],
  runners: [{
    agentId: 'helper',
    configRevision: 3,
    state: 'running',
    lastError: null,
  }],
}

describe('R4 collaboration runtime store', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    useCollabRuntimeStore.setState({ status: null, loading: false, error: null })
  })

  it('replaces the runtime projection with the canonical Server snapshot', async () => {
    vi.mocked(collabCommands.status).mockResolvedValue(status)
    await useCollabRuntimeStore.getState().fetch()
    expect(useCollabRuntimeStore.getState()).toMatchObject({
      status,
      loading: false,
      error: null,
    })
  })
})
