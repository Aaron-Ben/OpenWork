import { beforeEach, describe, expect, it } from 'vitest'

import { useRuntimeStore } from '../features/chat/runtimeStore'
import { useSessionStore } from '../features/sessions/sessionStore'
import { recordCoreBridgeError } from './useCoreEventBridge'

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
