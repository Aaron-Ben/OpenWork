import { beforeEach, describe, expect, it, vi } from 'vitest'

import { collabCommands, type CollabRun, type CollabRunTrace } from '@/bridge/collab'
import { useObservabilityStore } from './observabilityStore'

vi.mock('@/bridge/collab', () => ({
  collabCommands: {
    listRuns: vi.fn(),
    getRunTrace: vi.fn(),
  },
}))

const run: CollabRun = {
  id: 'run-1',
  agentId: 'helper',
  runtimeSessionId: 'runtime-1',
  trigger: 'message',
  status: 'completed',
  engineId: 'opencode',
  mainModelId: 'deepseek/v4-flash',
  observedModelId: 'deepseek/v4-flash',
  outcome: 'acted',
  roomId: 'room-1',
  focusCardId: null,
  triggerReason: null,
  errorCode: null,
  errorMessage: null,
  stage: 'run.completed',
  startedAt: '2026-09-01T10:00:00.000+08:00',
  heartbeatAt: '2026-09-01T10:00:01.000+08:00',
  endedAt: '2026-09-01T10:00:01.000+08:00',
  durationMs: 1_000,
  inputTokens: 10,
  cachedInputTokens: 2,
  cacheCreationInputTokens: 0,
  outputTokens: 4,
  rateLimitPercent: null,
  toolCalls: 1,
  eventCount: 5,
  inboxMessageCount: 1,
}

const trace: CollabRunTrace = { run, events: [] }

describe('collaboration observability store', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    useObservabilityStore.setState({
      runs: [],
      trace: null,
      selectedRunId: null,
      agentFilter: null,
      statusFilter: null,
      loading: false,
      error: null,
    })
  })

  it('selects the newest run and loads its complete trace', async () => {
    vi.mocked(collabCommands.listRuns).mockResolvedValue([run])
    vi.mocked(collabCommands.getRunTrace).mockResolvedValue(trace)

    await useObservabilityStore.getState().refresh()

    expect(collabCommands.listRuns).toHaveBeenCalledWith({ agentId: null, status: null })
    expect(collabCommands.getRunTrace).toHaveBeenCalledWith('run-1')
    expect(useObservabilityStore.getState()).toMatchObject({
      runs: [run],
      selectedRunId: 'run-1',
      trace,
      error: null,
    })
  })

  it('passes filters to the Server-owned run projection', async () => {
    useObservabilityStore.setState({ agentFilter: 'helper', statusFilter: 'failed' })
    vi.mocked(collabCommands.listRuns).mockResolvedValue([])

    await useObservabilityStore.getState().refresh()

    expect(collabCommands.listRuns).toHaveBeenCalledWith({
      agentId: 'helper',
      status: 'failed',
    })
  })
})
