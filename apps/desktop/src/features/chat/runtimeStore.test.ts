import { beforeEach, describe, expect, it } from 'vitest'

import type { RuntimeSessionUpdateEnvelope } from '../../bridge/compat'
import { useRuntimeStore } from './runtimeStore'

function textUpdate(sessionId: string, turnId: string, text: string): RuntimeSessionUpdateEnvelope {
  return {
    version: 1,
    sessionId,
    turnId,
    sequence: 1,
    occurredAtMs: 1,
    update: { type: 'text_delta', delta: text },
  }
}

describe('runtimeStore', () => {
  beforeEach(() => useRuntimeStore.setState({ bySession: {} }))

  it('allows two sessions to hold independent active turns', () => {
    const store = useRuntimeStore.getState()
    store.beginTurn('session-1', 'request-1', 'first')
    store.beginTurn('session-2', 'request-2', 'second')
    store.acceptTurn('session-1', 'request-1', 'turn-1')
    store.acceptTurn('session-2', 'request-2', 'turn-2')
    store.apply(textUpdate('session-1', 'turn-1', 'alpha'))
    store.apply(textUpdate('session-2', 'turn-2', 'beta'))

    const state = useRuntimeStore.getState().bySession
    expect(state['session-1']).toMatchObject({
      turnId: 'turn-1',
      pendingUserMessage: { text: 'first' },
      assistantDraft: { text: 'alpha' },
    })
    expect(state['session-2']).toMatchObject({
      turnId: 'turn-2',
      pendingUserMessage: { text: 'second' },
      assistantDraft: { text: 'beta' },
    })
  })

  it('clears only the reconciled session after canonical messages reload', () => {
    const store = useRuntimeStore.getState()
    store.beginTurn('session-1', 'request-1', 'first')
    store.beginTurn('session-2', 'request-2', 'second')
    store.reconcileCanonical('session-1')

    expect(useRuntimeStore.getState().bySession['session-1'].pendingUserMessage).toBeNull()
    expect(useRuntimeStore.getState().bySession['session-2'].pendingUserMessage?.text).toBe('second')
  })

  it('drops terminal turn state after canonical messages are reconciled', () => {
    const store = useRuntimeStore.getState()
    store.beginTurn('session-1', 'request-1', 'first')
    store.acceptTurn('session-1', 'request-1', 'turn-1')
    store.apply(textUpdate('session-1', 'turn-1', 'draft'))
    store.apply({
      version: 1,
      sessionId: 'session-1',
      turnId: 'turn-1',
      sequence: 2,
      occurredAtMs: 2,
      update: {
        type: 'tool_call_started',
        toolCall: {
          toolCallId: 'tool-1',
          providerCallId: 'call-1',
          name: 'read',
          input: { path: '/repo/README.md' },
          status: 'running',
          output: null,
          isError: null,
        },
      },
    })
    store.apply({
      version: 1,
      sessionId: 'session-1',
      turnId: 'turn-1',
      sequence: 3,
      occurredAtMs: 3,
      update: { type: 'turn_finished', outcome: { status: 'completed', finalText: 'done' } },
    })

    store.reconcileCanonical('session-1')

    expect(useRuntimeStore.getState().bySession['session-1']).toMatchObject({
      lastSequence: 3,
      turnId: null,
      clientRequestId: null,
      phase: 'idle',
      pendingUserMessage: null,
      assistantDraft: null,
      toolCalls: {},
      orderedToolCallIds: [],
      pendingPermission: null,
      terminal: null,
    })
  })
})
