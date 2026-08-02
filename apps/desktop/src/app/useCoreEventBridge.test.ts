import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import type { RuntimeSessionUpdateEnvelope } from '../bridge/compat'
import { useRuntimeStore } from '../features/chat/runtimeStore'
import { useSessionStore } from '../features/sessions/sessionStore'
import {
  createSessionUpdateDispatcher,
  recordCoreBridgeError,
} from './useCoreEventBridge'

function progressEnvelope(sequence: number): RuntimeSessionUpdateEnvelope {
  return {
    version: 1,
    sessionId: 'session-1',
    turnId: 'turn-1',
    sequence,
    occurredAtMs: sequence,
    update: {
      type: 'tool_call_progress',
      toolCallId: 'tool-1',
      progress: { kind: 'stdout', chunk: `line ${sequence}\n` },
    },
  }
}

function reasoningEnvelope(sequence: number): RuntimeSessionUpdateEnvelope {
  return {
    ...progressEnvelope(sequence),
    update: { type: 'reasoning_delta', delta: `thought ${sequence} ` },
  }
}

describe('recordCoreBridgeError', () => {
  beforeEach(() => {
    useRuntimeStore.setState({ bySession: {} })
    useSessionStore.setState({ error: null })
  })

  it('marks a session stale when processing one of its updates fails', () => {
    recordCoreBridgeError('session-1', new Error('invalid update'))

    expect(useRuntimeStore.getState().bySession['session-1']).toMatchObject({
      syncState: 'stale',
      error: 'invalid update',
    })
  })

  it('surfaces listener setup failures through the session error boundary', () => {
    recordCoreBridgeError(null, new Error('listener unavailable'))

    expect(useSessionStore.getState().error).toBe('listener unavailable')
  })
})

describe('createSessionUpdateDispatcher', () => {
  afterEach(() => vi.useRealTimers())

  it('publishes a burst of tool progress as one batch per window', () => {
    vi.useFakeTimers()
    const applyLiveBatch = vi.fn()
    const processUpdate = vi.fn().mockResolvedValue(undefined)
    const onError = vi.fn()
    const dispatcher = createSessionUpdateDispatcher({
      applyLiveBatch,
      processUpdate,
      onError,
    })

    for (let sequence = 1; sequence <= 200; sequence += 1) {
      dispatcher.dispatch(progressEnvelope(sequence))
    }

    expect(applyLiveBatch).not.toHaveBeenCalled()
    vi.advanceTimersByTime(99)
    expect(applyLiveBatch).not.toHaveBeenCalled()
    vi.advanceTimersByTime(1)

    expect(applyLiveBatch).toHaveBeenCalledOnce()
    expect(applyLiveBatch).toHaveBeenCalledWith(
      expect.arrayContaining([
        expect.objectContaining({ sequence: 1 }),
        expect.objectContaining({ sequence: 200 }),
      ]),
    )
    expect(vi.mocked(applyLiveBatch).mock.calls[0][0]).toHaveLength(200)
    expect(processUpdate).not.toHaveBeenCalled()
    expect(onError).not.toHaveBeenCalled()
    dispatcher.dispose()
  })

  it('publishes a burst of model reasoning as one batch per window', () => {
    vi.useFakeTimers()
    const applyLiveBatch = vi.fn()
    const processUpdate = vi.fn().mockResolvedValue(undefined)
    const dispatcher = createSessionUpdateDispatcher({
      applyLiveBatch,
      processUpdate,
      onError: vi.fn(),
    })

    for (let sequence = 1; sequence <= 341; sequence += 1) {
      dispatcher.dispatch(reasoningEnvelope(sequence))
    }

    expect(processUpdate).not.toHaveBeenCalled()
    vi.advanceTimersByTime(100)
    expect(applyLiveBatch).toHaveBeenCalledOnce()
    expect(vi.mocked(applyLiveBatch).mock.calls[0][0]).toHaveLength(341)
    dispatcher.dispose()
  })

  it('flushes pending progress before a non-progress update', () => {
    vi.useFakeTimers()
    const order: string[] = []
    const dispatcher = createSessionUpdateDispatcher({
      applyLiveBatch: () => { order.push('progress') },
      processUpdate: async () => { order.push('terminal') },
      onError: vi.fn(),
    })

    dispatcher.dispatch(progressEnvelope(1))
    dispatcher.dispatch({
      ...progressEnvelope(2),
      update: { type: 'turn_finished', outcome: { status: 'completed', finalText: 'done' } },
    })

    expect(order).toEqual(['progress', 'terminal'])
    dispatcher.dispose()
  })
})
