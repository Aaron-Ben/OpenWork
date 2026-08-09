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
        'parent-1': { tokens: 42_100, steps: 12 },
        a: { tokens: 31_600, steps: 2 },
      },
    }))

    expect(items[0]).toMatchObject({ status: 'running', tokens: 42_100, steps: 12, elapsedMs: 9_000 })
    expect(items[1]).toMatchObject({ status: 'completed', tokens: 31_600, steps: 2, elapsedMs: 3_000 })
  })

  it('treats a session with no traces yet as zero rather than blank', () => {
    const items = buildAgentRailItems(input({ totalsBySession: {} }))
    expect(items.every((item) => item.tokens === 0 && item.steps === 0)).toBe(true)
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
      'parent-1': { tokens: 42_100, steps: 12 },
      a: { tokens: 31_600, steps: 2 },
      b: { tokens: 96_400, steps: 8 },
    },
  }))

  it('counts running and standby agents, leaving terminal ones out of both', () => {
    expect(agentRailCounts(items)).toEqual({ running: 2, standby: 1 })
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
