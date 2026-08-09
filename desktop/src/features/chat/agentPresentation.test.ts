import { describe, expect, it } from 'vitest'

import {
  agentElapsedMs,
  agentInitial,
  agentStatus,
  agentSummary,
  agentToolActivity,
  formatElapsedClock,
  formatTokenCount,
} from './agentPresentation'
import { createSessionRuntimeView, type SessionRuntimeView } from './runtimeReducer'

function view(overrides: Partial<SessionRuntimeView>): SessionRuntimeView {
  return { ...createSessionRuntimeView(), ...overrides }
}

describe('agentStatus', () => {
  it('treats any non-idle phase as running even when a stale terminal is present', () => {
    const runtime = view({
      phase: 'running_tools',
      terminal: { status: 'completed', finalText: 'done' },
    })
    expect(agentStatus(runtime)).toBe('running')
  })

  it('falls back to the terminal outcome once the turn is idle', () => {
    expect(agentStatus(view({ terminal: { status: 'failed', code: 'E', message: 'boom' } }))).toBe('failed')
    expect(agentStatus(view({ terminal: { status: 'cancelled' } }))).toBe('cancelled')
    expect(agentStatus(view({}))).toBe('idle')
    expect(agentStatus(undefined)).toBe('idle')
  })
})

describe('agentSummary', () => {
  it('renders the final text and the failure code, and leaves cancellation to the caller', () => {
    expect(agentSummary(view({ terminal: { status: 'completed', finalText: 'ok' } }))).toBe('ok')
    expect(agentSummary(view({ terminal: { status: 'failed', code: 'E42', message: 'boom' } })))
      .toBe('E42: boom')
    expect(agentSummary(view({ terminal: { status: 'cancelled' } }))).toBeNull()
    expect(agentSummary(undefined)).toBeNull()
  })
})

describe('agentElapsedMs', () => {
  it('measures against now while running and against the end once finished', () => {
    expect(agentElapsedMs(view({ startedAtMs: 1_000 }), 4_000)).toBe(3_000)
    expect(agentElapsedMs(view({ startedAtMs: 1_000, endedAtMs: 2_500 }), 9_999)).toBe(1_500)
  })

  it('has no duration before the turn starts and never goes negative', () => {
    expect(agentElapsedMs(view({}), 5_000)).toBeNull()
    expect(agentElapsedMs(undefined, 5_000)).toBeNull()
    expect(agentElapsedMs(view({ startedAtMs: 5_000 }), 1_000)).toBe(0)
  })
})

describe('agentToolActivity', () => {
  it('reports the latest tool call and how many times that tool ran in the turn', () => {
    const runtime = view({
      orderedToolCallIds: ['a', 'b', 'c'],
      toolCalls: {
        a: { toolCallId: 'a', providerCallId: 'pa', name: 'python', input: null, status: 'ok', output: null, isError: false },
        b: { toolCallId: 'b', providerCallId: 'pb', name: 'read', input: null, status: 'ok', output: null, isError: false },
        c: { toolCallId: 'c', providerCallId: 'pc', name: 'python', input: null, status: 'running', output: null, isError: null },
      },
    })
    expect(agentToolActivity(runtime)).toEqual({ name: 'python', attempt: 2 })
  })

  it('has nothing to report before any tool runs', () => {
    expect(agentToolActivity(view({}))).toBeNull()
    expect(agentToolActivity(undefined)).toBeNull()
  })
})

describe('formatElapsedClock', () => {
  it('formats as mm:ss and grows an hour segment only when needed', () => {
    expect(formatElapsedClock(0)).toBe('00:00')
    expect(formatElapsedClock(108_000)).toBe('01:48')
    expect(formatElapsedClock(21_000)).toBe('00:21')
    expect(formatElapsedClock(3_661_000)).toBe('1:01:01')
  })

  it('shows a placeholder when there is no duration', () => {
    expect(formatElapsedClock(null)).toBe('--:--')
    expect(formatElapsedClock(-1)).toBe('--:--')
    expect(formatElapsedClock(Number.NaN)).toBe('--:--')
  })
})

describe('formatTokenCount', () => {
  it('switches units at a thousand and a million', () => {
    expect(formatTokenCount(0)).toBe('0')
    expect(formatTokenCount(999)).toBe('999')
    expect(formatTokenCount(1_234)).toBe('1.2k')
    expect(formatTokenCount(96_400)).toBe('96.4k')
    expect(formatTokenCount(184_200)).toBe('184.2k')
    expect(formatTokenCount(1_500_000)).toBe('1.5M')
  })

  it('renders missing counts as zero rather than NaN', () => {
    expect(formatTokenCount(null)).toBe('0')
    expect(formatTokenCount(undefined)).toBe('0')
  })
})

describe('agentInitial', () => {
  it('uses the first character of the role and falls back for empty roles', () => {
    expect(agentInitial('Researcher')).toBe('R')
    expect(agentInitial('  analyst')).toBe('A')
    expect(agentInitial('   ')).toBe('?')
  })
})
