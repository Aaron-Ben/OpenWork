import { describe, expect, it } from 'vitest'

import type { CollabRun } from '@/bridge/collab'
import { roomRunState } from './roomRunState'

function run(overrides: Partial<CollabRun> = {}): CollabRun {
  return {
    id: 'run-1',
    agentId: 'alpha',
    runtimeSessionId: 'runtime-test',
    trigger: 'message',
    status: 'running',
    engineId: 'opencode',
    mainModelId: 'deepseek/deepseek-v4-flash',
    outcome: null,
    roomId: 'room_1',
    focusCardId: null,
    triggerReason: null,
    errorCode: null,
    errorMessage: null,
    startedAt: '2026-08-30T19:00:00+08:00',
    ...overrides,
  }
}

describe('roomRunState', () => {
  it('shows active Agents as thinking', () => {
    expect(roomRunState([run()], 'room_1')).toEqual({
      kind: 'thinking', agentIds: ['alpha'], message: null,
    })
  })

  it('shows a new attempt after a rate limit as retrying', () => {
    expect(roomRunState([
      run({ id: 'run_retry', startedAt: '2026-08-30T19:01:00+08:00' }),
      run({ id: 'run_limited', status: 'failed', errorCode: 'ENGINE_RATE_LIMITED', startedAt: '2026-08-30T19:00:00+08:00' }),
    ], 'room_1')?.kind).toBe('retrying')
  })

  it('does not call an active run a retry because of an old rate limit', () => {
    expect(roomRunState([
      run({ id: 'run_active', startedAt: '2026-08-30T19:02:00+08:00' }),
      run({ id: 'run_success', status: 'completed', startedAt: '2026-08-30T19:01:00+08:00' }),
      run({ id: 'run_limited', status: 'failed', errorCode: 'ENGINE_RATE_LIMITED', startedAt: '2026-08-30T19:00:00+08:00' }),
    ], 'room_1')?.kind).toBe('thinking')
  })

  it('recognizes a terminal Provider rate limit', () => {
    expect(roomRunState([
      run({ status: 'failed', errorMessage: 'Rate limit exceeded. Please try again later.' }),
    ], 'room_1')).toEqual({
      kind: 'rate_limited', agentIds: ['alpha'], message: 'Rate limit exceeded. Please try again later.',
    })
  })

  it('clears the status after the latest run completes', () => {
    expect(roomRunState([run({ status: 'completed', outcome: 'acted' })], 'room_1')).toBeNull()
  })
})
