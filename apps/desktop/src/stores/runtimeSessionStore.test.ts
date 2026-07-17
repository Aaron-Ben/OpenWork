import { beforeEach, describe, expect, it, vi } from 'vitest'

import { runtimeApi } from '../api/runtime'
import type { ProviderConfig } from '../type/providers'
import type { RuntimeSessionUpdateEnvelope } from '../type/runtime'
import { useRuntimeSessionStore } from './runtimeSessionStore'

vi.mock('../api/runtime', () => ({
  runtimeApi: {
    upsertModel: vi.fn(),
    listSessions: vi.fn(),
    createSession: vi.fn(),
    loadSession: vi.fn(),
    renameSession: vi.fn(),
    deleteSession: vi.fn(),
    startTurn: vi.fn(),
    cancelTurn: vi.fn(),
    resolvePermission: vi.fn(),
    snapshot: vi.fn(),
    replayUpdates: vi.fn(),
    listTraces: vi.fn(),
    getTrace: vi.fn(),
    listenToUpdates: vi.fn(),
  },
}))

const provider: ProviderConfig = {
  id: 'provider-deepseek',
  name: 'DeepSeek',
  baseUrl: 'https://api.deepseek.com',
  kind: 'deepseek',
  enabled: true,
  models: [
    {
      modelId: 'deepseek-v4-flash',
      displayName: 'DeepSeek V4 Flash',
      modelTier: 'plus',
      enabled: true,
    },
  ],
}

beforeEach(() => {
  vi.clearAllMocks()
  useRuntimeSessionStore.setState({
    sessions: [],
    activeSessionId: null,
    messagesBySession: {},
    lastSequenceBySession: {},
    activeTurn: null,
    isLoading: false,
    error: null,
  })
})

describe('runtimeSessionStore', () => {
  it('bridges an encrypted provider reference into a selectable runtime model', async () => {
    vi.mocked(runtimeApi.upsertModel).mockResolvedValue()
    vi.mocked(runtimeApi.createSession).mockImplementation(async (input) => ({
      ...input,
      title: input.title ?? null,
      defaultModelId: input.defaultModelId ?? null,
      status: 'active',
      createdAt: '2026-07-18T00:00:00Z',
      updatedAt: '2026-07-18T00:00:00Z',
      lastTurnAt: null,
    }))

    const id = await useRuntimeSessionStore.getState().create({
      title: 'Session',
      workingDirectory: '/repo',
      provider,
      modelId: 'deepseek-v4-flash',
    })

    expect(id).toMatch(/^sess-/)
    expect(runtimeApi.upsertModel).toHaveBeenCalledWith(
      expect.objectContaining({
        providerKind: 'deepseek',
        modelName: 'deepseek-v4-flash',
        credentialRef: 'provider:provider-deepseek',
      }),
    )
    expect(runtimeApi.createSession).toHaveBeenCalledWith(
      expect.objectContaining({
        workingDirectory: '/repo',
        defaultModelId: 'model:provider-deepseek:deepseek-v4-flash',
      }),
    )
  })

  it('replays missing sequence numbers before applying the latest delta', async () => {
    const first: RuntimeSessionUpdateEnvelope = {
      version: 1,
      sessionId: 'session-1',
      turnId: 'turn-1',
      sequence: 1,
      occurredAtMs: 1,
      update: { type: 'text_delta', delta: 'hello ' },
    }
    const second: RuntimeSessionUpdateEnvelope = {
      ...first,
      sequence: 2,
      occurredAtMs: 2,
      update: { type: 'text_delta', delta: 'world' },
    }
    vi.mocked(runtimeApi.replayUpdates).mockResolvedValue([first])

    await useRuntimeSessionStore.getState().ingestUpdate(second)

    expect(runtimeApi.replayUpdates).toHaveBeenCalledWith('session-1', 0)
    expect(useRuntimeSessionStore.getState().lastSequenceBySession['session-1']).toBe(2)
    expect(useRuntimeSessionStore.getState().messagesBySession['session-1']).toEqual([
      expect.objectContaining({
        id: 'live-turn-1',
        parts: [{ type: 'text', text: 'hello world' }],
      }),
    ])
  })
})
