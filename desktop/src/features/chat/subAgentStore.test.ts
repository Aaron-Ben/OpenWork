import { afterEach, describe, expect, it, vi } from 'vitest'

import { coreCommands } from '@/bridge/commands'
import type { RuntimeSubAgentSessionRecord, RuntimeTraceSummary } from '@/bridge/compat'
import { useRuntimeStore } from './runtimeStore'
import {
  resetSubAgentPolling,
  retainSubAgents,
  sessionTraceTotals,
  setSubAgentPolling,
  subAgentPollingActive,
  subAgentPollingRefCount,
  useSubAgentStore,
} from './subAgentStore'

function child(id: string, overrides: Partial<RuntimeSubAgentSessionRecord> = {}): RuntimeSubAgentSessionRecord {
  return {
    id,
    title: null,
    workingDirectory: '/repo',
    defaultModelId: 'model-1',
    status: 'active',
    createdAt: '2026-08-08T12:00:00+08:00',
    updatedAt: '2026-08-08T12:00:05+08:00',
    lastTurnAt: '2026-08-08T12:00:05+08:00',
    parentSessionId: 'parent-1',
    taskName: `task_${id}`,
    agentRole: 'explorer',
    spawnSpanId: null,
    ...overrides,
  }
}

function trace(totalTokens: number): RuntimeTraceSummary {
  return {
    traceId: `trace-${totalTokens}`,
    turnId: 'turn-1',
    sessionId: 'session-1',
    turnSequence: 1,
    status: 'succeeded',
    resolvedModelName: 'model-1',
    modelCallCount: 1,
    modelSubmissionCount: 1,
    toolCallCount: 2,
    spanCount: 3,
    totalTokens,
    startedAt: '2026-08-08T12:00:00+08:00',
    endedAt: '2026-08-08T12:00:05+08:00',
  }
}

afterEach(() => {
  resetSubAgentPolling()
  useRuntimeStore.setState({ bySession: {}, subAgentParentBySession: {} })
  vi.restoreAllMocks()
  vi.useRealTimers()
})

describe('useSubAgentStore.refresh', () => {
  it('stores the listed children and registers them on the runtime store', async () => {
    vi.spyOn(coreCommands, 'listSubAgents').mockResolvedValue([child('a'), child('b')])

    await useSubAgentStore.getState().refresh('parent-1')

    expect(useSubAgentStore.getState().byParent['parent-1'].children.map((item) => item.id))
      .toEqual(['a', 'b'])
    expect(useRuntimeStore.getState().subAgentParentBySession).toEqual({
      a: 'parent-1',
      b: 'parent-1',
    })
  })

  it('keeps the last successful list when a refresh fails', async () => {
    const listSubAgents = vi.spyOn(coreCommands, 'listSubAgents')
    listSubAgents.mockResolvedValueOnce([child('a')])
    await useSubAgentStore.getState().refresh('parent-1')

    listSubAgents.mockRejectedValueOnce(new Error('bridge down'))
    await useSubAgentStore.getState().refresh('parent-1')

    const entry = useSubAgentStore.getState().byParent['parent-1']
    expect(entry.children.map((item) => item.id)).toEqual(['a'])
    expect(entry.error).toContain('bridge down')
  })

  it('discards a stale response that resolves after a newer one', async () => {
    let releaseFirst: (value: RuntimeSubAgentSessionRecord[]) => void = () => undefined
    const first = new Promise<RuntimeSubAgentSessionRecord[]>((resolve) => {
      releaseFirst = resolve
    })
    vi.spyOn(coreCommands, 'listSubAgents')
      .mockReturnValueOnce(first)
      .mockResolvedValueOnce([child('new')])

    const stale = useSubAgentStore.getState().refresh('parent-1')
    await useSubAgentStore.getState().refresh('parent-1')
    releaseFirst([child('old')])
    await stale

    expect(useSubAgentStore.getState().byParent['parent-1'].children.map((item) => item.id))
      .toEqual(['new'])
  })
})

describe('useSubAgentStore.refreshTotals', () => {
  it('sums tokens and steps across every trace of the parent and of each child', async () => {
    vi.spyOn(coreCommands, 'listSubAgents').mockResolvedValue([child('a')])
    vi.spyOn(coreCommands, 'listTraces').mockImplementation(async (sessionId) => (
      sessionId === 'parent-1' ? [trace(1_000), trace(500)] : [trace(96_400)]
    ))

    await useSubAgentStore.getState().refresh('parent-1')
    await useSubAgentStore.getState().refreshTotals('parent-1')

    expect(useSubAgentStore.getState().byParent['parent-1'].totalsBySession).toEqual({
      'parent-1': { tokens: 1_500, steps: 6 },
      a: { tokens: 96_400, steps: 3 },
    })
  })

  it('leaves the previous totals in place when the trace read fails', async () => {
    const listTraces = vi.spyOn(coreCommands, 'listTraces')
    listTraces.mockResolvedValueOnce([trace(400)])
    await useSubAgentStore.getState().refreshTotals('parent-1')

    listTraces.mockRejectedValueOnce(new Error('unavailable'))
    await useSubAgentStore.getState().refreshTotals('parent-1')

    expect(useSubAgentStore.getState().byParent['parent-1'].totalsBySession)
      .toEqual({ 'parent-1': { tokens: 400, steps: 3 } })
  })
})

describe('sessionTraceTotals', () => {
  it('treats a session without traces as zero rather than missing', async () => {
    vi.spyOn(coreCommands, 'listTraces').mockResolvedValue([])
    await expect(sessionTraceTotals('empty')).resolves.toEqual({ tokens: 0, steps: 0 })
  })
})

describe('sub agent polling', () => {
  it('runs a single timer no matter how many consumers subscribe', async () => {
    vi.useFakeTimers()
    const listSubAgents = vi.spyOn(coreCommands, 'listSubAgents').mockResolvedValue([])
    vi.spyOn(coreCommands, 'listTraces').mockResolvedValue([])

    const releaseFirst = retainSubAgents('parent-1')
    const releaseSecond = retainSubAgents('parent-1')
    expect(subAgentPollingRefCount('parent-1')).toBe(2)

    setSubAgentPolling('parent-1', true)
    expect(subAgentPollingActive('parent-1')).toBe(true)

    listSubAgents.mockClear()
    await vi.advanceTimersByTimeAsync(1_000)
    expect(listSubAgents).toHaveBeenCalledTimes(1)

    releaseFirst()
    expect(subAgentPollingActive('parent-1')).toBe(true)

    releaseSecond()
    expect(subAgentPollingRefCount('parent-1')).toBe(0)
    expect(subAgentPollingActive('parent-1')).toBe(false)
  })

  it('stops polling once the parent turn goes idle', async () => {
    vi.useFakeTimers()
    const listSubAgents = vi.spyOn(coreCommands, 'listSubAgents').mockResolvedValue([])
    vi.spyOn(coreCommands, 'listTraces').mockResolvedValue([])

    retainSubAgents('parent-1')
    setSubAgentPolling('parent-1', true)
    setSubAgentPolling('parent-1', false)

    expect(subAgentPollingActive('parent-1')).toBe(false)
    listSubAgents.mockClear()
    await vi.advanceTimersByTimeAsync(5_000)
    expect(listSubAgents).not.toHaveBeenCalled()
  })

  it('ignores a release that fires twice', () => {
    vi.spyOn(coreCommands, 'listSubAgents').mockResolvedValue([])
    vi.spyOn(coreCommands, 'listTraces').mockResolvedValue([])

    const release = retainSubAgents('parent-1')
    retainSubAgents('parent-1')
    release()
    release()

    expect(subAgentPollingRefCount('parent-1')).toBe(1)
  })
})
