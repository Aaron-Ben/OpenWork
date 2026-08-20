import { beforeEach, describe, expect, it, vi } from 'vitest'

import { collabCommands, type CollabLogEntry } from '@/bridge/collab'
import { useLogStore } from './logStore'

vi.mock('@/bridge/collab', () => ({ collabCommands: { listLogs: vi.fn() } }))

const entry: CollabLogEntry = {
  source: 'event',
  id: 'evt_1',
  runId: 'run_1',
  agentId: 'alice',
  roomId: 'general',
  kind: 'prompt.started',
  payload: { trigger: 'message' },
  createdAt: '2026-08-19T10:00:00+08:00',
}

describe('logStore', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    useLogStore.setState({ entries: [], roomId: null, loading: false, error: null })
  })

  it('loads one bounded flat timeline without deriving collaboration semantics', async () => {
    vi.mocked(collabCommands.listLogs).mockResolvedValue([entry])
    await useLogStore.getState().fetch('general')
    expect(collabCommands.listLogs).toHaveBeenCalledWith('general', 300)
    expect(useLogStore.getState().entries).toEqual([entry])
    expect(useLogStore.getState().roomId).toBe('general')
  })
})
