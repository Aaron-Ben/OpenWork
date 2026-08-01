import { beforeEach, describe, expect, it, vi } from 'vitest'

import {
  RUNTIME_SESSION_UPDATE_VERSION,
  type RuntimeSessionSnapshot,
  type RuntimeSessionUpdateEnvelope,
} from '../bridge/compat'
import { useRuntimeStore } from '../features/chat/runtimeStore'
import {
  processSessionUpdate,
  resyncSessionView,
  type CoreEventControllerDependencies,
} from './coreEventController'

function envelope(sequence: number, delta: string): RuntimeSessionUpdateEnvelope {
  return {
    version: 3,
    sessionId: 'session-1',
    turnId: 'turn-1',
    sequence,
    occurredAtMs: sequence,
    update: { type: 'text_delta', delta },
  }
}

function dependencies(): CoreEventControllerDependencies {
  return {
    apply: vi.fn((payload) => useRuntimeStore.getState().apply(payload)),
    getLastSequence: (sessionId) => useRuntimeStore.getState().bySession[sessionId]?.lastSequence ?? 0,
    markResyncing: vi.fn(),
    markSyncFailed: vi.fn(),
    replaceSnapshot: vi.fn(),
    replayUpdates: vi.fn().mockResolvedValue([]),
    loadSnapshot: vi.fn(),
    reloadCanonical: vi.fn().mockResolvedValue(true),
    reconcileCanonical: vi.fn(),
    refreshSessions: vi.fn().mockResolvedValue(undefined),
  }
}

describe('coreEventController', () => {
  beforeEach(() => useRuntimeStore.setState({ bySession: {} }))

  it('replays missing updates before applying the event that exposed the gap', async () => {
    const deps = dependencies()
    vi.mocked(deps.replayUpdates).mockResolvedValue([envelope(1, 'hello ')])

    await processSessionUpdate(envelope(2, 'world'), deps)

    expect(deps.replayUpdates).toHaveBeenCalledWith('session-1', 0)
    expect(useRuntimeStore.getState().bySession['session-1'].assistantDraft?.text).toBe('hello world')
  })

  it('refreshes canonical messages and session summaries only after a terminal event', async () => {
    const deps = dependencies()
    const terminal: RuntimeSessionUpdateEnvelope = {
      ...envelope(1, ''),
      update: { type: 'turn_finished', outcome: { status: 'completed', finalText: 'done' } },
    }

    await processSessionUpdate(terminal, deps)

    expect(deps.reloadCanonical).toHaveBeenCalledWith('session-1')
    expect(deps.reconcileCanonical).toHaveBeenCalledWith('session-1')
    expect(deps.refreshSessions).toHaveBeenCalledOnce()
  })

  it('marks the session stale with a visible error when snapshot resync fails', async () => {
    const deps = dependencies()
    vi.mocked(deps.loadSnapshot).mockRejectedValue(new Error('snapshot unavailable'))

    await resyncSessionView('session-1', deps)

    expect(deps.markResyncing).toHaveBeenCalledWith('session-1')
    expect(deps.markSyncFailed).toHaveBeenCalledWith('session-1', 'snapshot unavailable')
  })

  it('refreshes canonical messages after a snapshot resync without reconciling a running draft', async () => {
    const deps = dependencies()
    const snapshot: RuntimeSessionSnapshot = {
      version: 1,
      sessionId: 'session-1',
      lastUpdateSequence: 4,
      permissionMode: 'default',
      runtime: {
        state: 'running',
        turnId: 'turn-1',
        clientRequestId: 'request-1',
        phase: 'running_model',
        draftText: 'partial',
        draftReasoning: '',
        toolCalls: [],
        pendingPermission: null,
      },
    }
    vi.mocked(deps.loadSnapshot).mockResolvedValue(snapshot)

    await resyncSessionView('session-1', deps)

    expect(deps.replaceSnapshot).toHaveBeenCalledWith(snapshot)
    expect(deps.reloadCanonical).toHaveBeenCalledWith('session-1')
    expect(deps.reconcileCanonical).not.toHaveBeenCalled()
  })

  it('does not apply an unsupported update version and leaves an upgrade error visible', async () => {
    const deps = dependencies()
    const unsupportedVersion = RUNTIME_SESSION_UPDATE_VERSION + 1
    const snapshot: RuntimeSessionSnapshot = {
      version: 1,
      sessionId: 'session-1',
      lastUpdateSequence: 0,
      permissionMode: 'default',
      runtime: { state: 'idle' },
    }
    vi.mocked(deps.loadSnapshot).mockResolvedValue(snapshot)

    await processSessionUpdate({ ...envelope(1, 'ignored'), version: unsupportedVersion }, deps)

    expect(deps.replaceSnapshot).toHaveBeenCalledWith(snapshot)
    expect(deps.apply).not.toHaveBeenCalled()
    expect(deps.markSyncFailed).toHaveBeenLastCalledWith(
      'session-1',
      `Unsupported session update version: ${unsupportedVersion}`,
    )
  })
})
