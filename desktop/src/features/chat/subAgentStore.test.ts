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

function trace(
  totalTokens: number,
  overrides: Partial<RuntimeTraceSummary> = {},
): RuntimeTraceSummary {
  return {
    traceId: `trace-${totalTokens}`,
    turnId: 'turn-1',
    sessionId: 'session-1',
    turnSequence: 1,
    status: 'completed',
    resolvedModelName: 'model-1',
    modelCallCount: 1,
    modelSubmissionCount: 1,
    toolCallCount: 2,
    spanCount: 3,
    totalTokens,
    startedAt: '2026-08-08T12:00:00+08:00',
    endedAt: '2026-08-08T12:00:05+08:00',
    ...overrides,
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
      'parent-1': {
        tokens: 1_500, steps: 6, latestTurnStatus: 'completed',
        runtimeMs: 10_000, latestTurnId: 'turn-1', latestTurnMs: 10_000,
      },
      a: {
        tokens: 96_400, steps: 3, latestTurnStatus: 'completed',
        runtimeMs: 5_000, latestTurnId: 'turn-1', latestTurnMs: 5_000,
      },
    })
  })

  it('leaves the previous totals in place when the trace read fails', async () => {
    const listTraces = vi.spyOn(coreCommands, 'listTraces')
    listTraces.mockResolvedValueOnce([trace(400)])
    await useSubAgentStore.getState().refreshTotals('parent-1')

    listTraces.mockRejectedValueOnce(new Error('unavailable'))
    await useSubAgentStore.getState().refreshTotals('parent-1')

    expect(useSubAgentStore.getState().byParent['parent-1'].totalsBySession)
      .toEqual({ 'parent-1': {
        tokens: 400, steps: 3, latestTurnStatus: 'completed',
        runtimeMs: 5_000, latestTurnId: 'turn-1', latestTurnMs: 5_000,
      } })
  })
})

describe('sessionTraceTotals', () => {
  it('treats a session without traces as zero rather than missing', async () => {
    vi.spyOn(coreCommands, 'listTraces').mockResolvedValue([])
    await expect(sessionTraceTotals('empty')).resolves.toEqual({
      tokens: 0,
      steps: 0,
      latestTurnStatus: null,
      runtimeMs: 0,
      latestTurnId: null,
      latestTurnMs: 0,
    })
  })

  it('uses the newest turn trace status and ignores traces without a turn', async () => {
    vi.spyOn(coreCommands, 'listTraces').mockResolvedValue([
      trace(10, { traceId: 'manual', turnId: null, turnSequence: 99, status: 'failed' }),
      trace(20, { traceId: 'newer', turnId: 'turn-2', turnSequence: 2, status: 'failed' }),
      trace(30, { traceId: 'older', turnId: 'turn-1', turnSequence: 1, status: 'completed' }),
    ])

    await expect(sessionTraceTotals('session-1')).resolves.toEqual({
      tokens: 60,
      steps: 9,
      latestTurnStatus: 'failed',
      runtimeMs: 15_000,
      latestTurnId: 'turn-2',
      latestTurnMs: 5_000,
    })
  })

  it('accumulates execution time rather than spanning the idle gaps between turns', async () => {
    vi.spyOn(coreCommands, 'listTraces').mockResolvedValue([
      trace(10, {
        traceId: 'first',
        turnId: 'turn-1',
        turnSequence: 1,
        startedAt: '2026-08-08T12:00:00+08:00',
        endedAt: '2026-08-08T12:00:04+08:00',
      }),
      // 用户在这中间想了一分钟：墙钟跨度是 64s，实际执行只有 6s。
      trace(20, {
        traceId: 'second',
        turnId: 'turn-2',
        turnSequence: 2,
        startedAt: '2026-08-08T12:01:02+08:00',
        endedAt: '2026-08-08T12:01:04+08:00',
      }),
    ])

    const totals = await sessionTraceTotals('session-1')
    expect(totals.runtimeMs).toBe(6_000)
    expect(totals.latestTurnId).toBe('turn-2')
    expect(totals.latestTurnMs).toBe(2_000)
  })

  it('counts a running trace as zero so the live view can own the in-flight turn', async () => {
    vi.spyOn(coreCommands, 'listTraces').mockResolvedValue([
      trace(10, { traceId: 'done', turnId: 'turn-1', turnSequence: 1 }),
      trace(20, { traceId: 'live', turnId: 'turn-2', turnSequence: 2, status: 'running', endedAt: null }),
    ])

    const totals = await sessionTraceTotals('session-1')
    expect(totals.runtimeMs).toBe(5_000)
    expect(totals.latestTurnMs).toBe(0)
  })

  it('sums every trace belonging to the same turn', async () => {
    vi.spyOn(coreCommands, 'listTraces').mockResolvedValue([
      trace(10, {
        traceId: 'turn-2-a',
        turnId: 'turn-2',
        turnSequence: 2,
        startedAt: '2026-08-08T12:00:00+08:00',
        endedAt: '2026-08-08T12:00:03+08:00',
      }),
      trace(20, {
        traceId: 'turn-2-b',
        turnId: 'turn-2',
        turnSequence: 2,
        startedAt: '2026-08-08T12:00:03+08:00',
        endedAt: '2026-08-08T12:00:07+08:00',
      }),
    ])

    const totals = await sessionTraceTotals('session-1')
    expect(totals.latestTurnMs).toBe(7_000)
    expect(totals.runtimeMs).toBe(7_000)
  })

  it('ignores traces with no turn when attributing the newest turn duration', async () => {
    vi.spyOn(coreCommands, 'listTraces').mockResolvedValue([
      trace(10, { traceId: 'manual-compaction', turnId: null, turnSequence: null }),
      trace(20, { traceId: 'turn', turnId: 'turn-1', turnSequence: 1 }),
    ])

    const totals = await sessionTraceTotals('session-1')
    // 无归属的操作仍然占用了机器时间，计入总量；只是不能归给某个 Turn。
    expect(totals.runtimeMs).toBe(10_000)
    expect(totals.latestTurnId).toBe('turn-1')
    expect(totals.latestTurnMs).toBe(5_000)
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
