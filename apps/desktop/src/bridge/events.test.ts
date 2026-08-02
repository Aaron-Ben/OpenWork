import { beforeEach, describe, expect, it, vi } from 'vitest'

const listenMock = vi.hoisted(() => vi.fn())

vi.mock('@tauri-apps/api/event', () => ({ listen: listenMock }))

import type { RuntimeSessionUpdateEnvelope } from './compat'
import {
  listenToSessionUpdates,
  SESSION_UPDATE_BATCH_EVENT,
  SESSION_UPDATE_EVENT,
} from './events'

function envelope(sequence: number): RuntimeSessionUpdateEnvelope {
  return {
    version: 6,
    sessionId: 'session-1',
    turnId: 'turn-1',
    sequence,
    occurredAtMs: sequence,
    update: { type: 'phase_changed', phase: 'running_tools' },
  }
}

describe('Core event contract', () => {
  beforeEach(() => {
    listenMock.mockReset()
  })

  it('matches the process-level event names emitted by the Tauri host', () => {
    expect(SESSION_UPDATE_EVENT).toBe('openwork://session-update')
    expect(SESSION_UPDATE_BATCH_EVENT).toBe('openwork://session-update-batch')
  })

  it('fans a host-side batch out in original sequence order', async () => {
    const listeners = new Map<string, (event: { payload: unknown }) => void>()
    const unlistenSingle = vi.fn()
    const unlistenBatch = vi.fn()
    listenMock.mockImplementation(async (name, listener) => {
      listeners.set(name, listener)
      return name === SESSION_UPDATE_EVENT ? unlistenSingle : unlistenBatch
    })
    const received: number[] = []

    const unlisten = await listenToSessionUpdates((payload) => {
      received.push(payload.sequence)
    })
    listeners.get(SESSION_UPDATE_BATCH_EVENT)?.({ payload: [envelope(1), envelope(2)] })
    listeners.get(SESSION_UPDATE_EVENT)?.({ payload: envelope(3) })

    expect(received).toEqual([1, 2, 3])
    unlisten()
    expect(unlistenSingle).toHaveBeenCalledOnce()
    expect(unlistenBatch).toHaveBeenCalledOnce()
  })
})
