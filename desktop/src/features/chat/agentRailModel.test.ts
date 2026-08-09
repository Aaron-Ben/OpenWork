import { describe, expect, it } from 'vitest'

import type { RuntimeSubAgentSessionRecord } from '@/bridge/compat'
import {
  agentRailCounts,
  agentRailElapsedMs,
  agentRailTotalSteps,
  agentRailTotalTokens,
  buildAgentRailItems,
  findAgentRailItem,
  type AgentRailInput,
} from './agentRailModel'
import { createSessionRuntimeView, type SessionRuntimeView } from './runtimeReducer'
import { EMPTY_TRACE_TOTALS, type SessionTraceTotals } from './subAgentStore'

function child(id: string, role: string, task: string): RuntimeSubAgentSessionRecord {
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
    taskName: task,
    agentRole: role,
    spawnSpanId: null,
  }
}

function totals(overrides: Partial<SessionTraceTotals> = {}): SessionTraceTotals {
  return { ...EMPTY_TRACE_TOTALS, ...overrides }
}

function view(overrides: Partial<SessionRuntimeView>): SessionRuntimeView {
  return { ...createSessionRuntimeView(), ...overrides }
}

function input(overrides: Partial<AgentRailInput> = {}): AgentRailInput {
  return {
    parentSessionId: 'parent-1',
    orchestratorRole: '主控',
    orchestratorTask: 'Q3 渠道 ROI 异常复盘',
    children: [child('a', 'Researcher', 'fetch_channel_spend')],
    runtimeBySession: {},
    totalsBySession: {},
    nowMs: 10_000,
    ...overrides,
  }
}

describe('buildAgentRailItems', () => {
  it('always puts the orchestrator first and keeps the listed child order', () => {
    const items = buildAgentRailItems(input({
      children: [
        child('a', 'Researcher', 'fetch_channel_spend'),
        child('b', 'Analyst', 'compute_roi'),
      ],
    }))

    expect(items.map((item) => item.sessionId)).toEqual(['parent-1', 'a', 'b'])
    expect(items[0]).toMatchObject({ orchestrator: true, role: '主控', task: 'Q3 渠道 ROI 异常复盘' })
    expect(items[1]).toMatchObject({ orchestrator: false, role: 'Researcher', task: 'fetch_channel_spend' })
  })

  it('carries each session runtime state and trace totals onto its card', () => {
    const items = buildAgentRailItems(input({
      runtimeBySession: {
        'parent-1': view({ phase: 'running_tools', startedAtMs: 1_000 }),
        a: view({ terminal: { status: 'completed', finalText: 'done' }, startedAtMs: 2_000, endedAtMs: 5_000 }),
      },
      totalsBySession: {
        'parent-1': totals({ tokens: 42_100, steps: 12, latestTurnStatus: null }),
        a: totals({ tokens: 31_600, steps: 2, latestTurnStatus: null }),
      },
    }))

    expect(items[0]).toMatchObject({ status: 'running', tokens: 42_100, steps: 12, elapsedMs: 9_000 })
    expect(items[1]).toMatchObject({ status: 'completed', tokens: 31_600, steps: 2, elapsedMs: 3_000 })
  })

  it('keeps the duration after a restart drops the live runtime views', () => {
    const items = buildAgentRailItems(input({
      runtimeBySession: {},
      totalsBySession: {
        'parent-1': totals({ runtimeMs: 47_000, latestTurnId: 'turn-3', latestTurnMs: 9_000 }),
        a: totals(),
      },
    }))

    expect(items[0].elapsedMs).toBe(47_000)
    // 从没跑过的智能体是 --:--，不是 00:00。
    expect(items[1].elapsedMs).toBeNull()
  })

  it('treats a session with no traces yet as zero rather than blank', () => {
    const items = buildAgentRailItems(input({ totalsBySession: {} }))
    expect(items.every((item) => item.tokens === 0 && item.steps === 0)).toBe(true)
  })

  it('falls back to the canonical failed status after runtime reconciliation', () => {
    const items = buildAgentRailItems(input({
      runtimeBySession: { 'parent-1': view({ phase: 'idle', terminal: null }) },
      totalsBySession: {
        'parent-1': totals({ tokens: 42, steps: 3, latestTurnStatus: 'failed' }),
      },
    }))

    expect(items[0].status).toBe('failed')
  })

  it('uses the canonical failed status when no runtime view exists after restart', () => {
    const items = buildAgentRailItems(input({
      runtimeBySession: {},
      totalsBySession: {
        'parent-1': totals({ tokens: 42, steps: 3, latestTurnStatus: 'failed' }),
      },
    }))

    expect(items[0].status).toBe('failed')
  })

  it('does not apply the parent canonical fallback to child cards', () => {
    const items = buildAgentRailItems(input({
      runtimeBySession: {},
      totalsBySession: {
        a: totals({ tokens: 42, steps: 3, latestTurnStatus: 'failed' }),
      },
    }))

    expect(items[1].status).toBe('idle')
  })
})

describe('agent rail aggregates', () => {
  const items = buildAgentRailItems(input({
    children: [
      child('a', 'Researcher', 'fetch'),
      child('b', 'Analyst', 'compute'),
      child('c', 'Reviewer', 'review'),
    ],
    runtimeBySession: {
      'parent-1': view({ phase: 'running_model', startedAtMs: 1_000 }),
      a: view({ terminal: { status: 'completed', finalText: 'ok' }, startedAtMs: 2_000, endedAtMs: 4_000 }),
      b: view({ phase: 'running_tools', startedAtMs: 3_000 }),
    },
    totalsBySession: {
      'parent-1': totals({ tokens: 42_100, steps: 12, latestTurnStatus: null }),
      a: totals({ tokens: 31_600, steps: 2, latestTurnStatus: null }),
      b: totals({ tokens: 96_400, steps: 8, latestTurnStatus: null }),
    },
  }))

  it('counts idle and completed agents as standby without hiding failed ones in that total', () => {
    const terminalItems = [
      { ...items[0], sessionId: 'failed', status: 'failed' as const },
      { ...items[0], sessionId: 'cancelled', status: 'cancelled' as const },
    ]

    expect(agentRailCounts([...items, ...terminalItems])).toEqual({ running: 2, standby: 2 })
  })

  it('sums tokens and steps across the whole tree', () => {
    expect(agentRailTotalTokens(items)).toBe(170_100)
    expect(agentRailTotalSteps(items)).toBe(22)
  })

  it('reports the longest running agent as the tree duration', () => {
    expect(agentRailElapsedMs(items)).toBe(9_000)
    expect(agentRailElapsedMs([])).toBeNull()
  })
})

describe('findAgentRailItem', () => {
  const items = buildAgentRailItems(input())

  it('looks a card up by session id', () => {
    expect(findAgentRailItem(items, 'a')?.role).toBe('Researcher')
  })

  it('returns null for no selection or for an agent that has gone away', () => {
    expect(findAgentRailItem(items, null)).toBeNull()
    expect(findAgentRailItem(items, 'removed')).toBeNull()
  })
})
