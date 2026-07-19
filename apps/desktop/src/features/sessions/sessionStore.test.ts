import { beforeEach, describe, expect, it, vi } from 'vitest'

import { coreCommands } from '../../bridge/commands'
import type { ProviderConfig } from '../models/contracts'
import { useRuntimeStore } from '../chat/runtimeStore'
import { useSessionStore } from './sessionStore'

vi.mock('../../bridge/commands', () => ({
  coreCommands: {
    listSessions: vi.fn(),
    createSession: vi.fn(),
    loadSession: vi.fn(),
    renameSession: vi.fn(),
    deleteSession: vi.fn(),
  },
}))

const provider: ProviderConfig = {
  id: 'provider-deepseek',
  name: 'DeepSeek',
  baseUrl: 'https://api.deepseek.com',
  kind: 'deepseek',
  enabled: true,
  models: [{ modelId: 'deepseek-v4-flash', displayName: 'V4 Flash', modelTier: 'plus', enabled: true }],
}

describe('sessionStore', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    useSessionStore.setState({
      summaries: {},
      orderedSessionIds: [],
      activeSessionId: null,
      messagesBySession: {},
      loadStateBySession: {},
      isLoading: false,
      error: null,
    })
    useRuntimeStore.setState({ bySession: {} })
  })

  it('stores canonical messages without streaming fields', async () => {
    vi.mocked(coreCommands.loadSession).mockResolvedValue({
      session: {
        id: 'session-1', title: 'Session', workingDirectory: '/repo', defaultModelId: null,
        status: 'active', createdAt: '2026-07-18T00:00:00Z', updatedAt: '2026-07-18T00:00:00Z', lastTurnAt: null,
      },
      messages: [{
        id: 'message-1', turnId: 'turn-1', sequence: 1, role: 'assistant',
        content: [{ type: 'text', text: 'done' }], createdAt: '2026-07-18T00:00:01Z',
      }],
    })

    expect(await useSessionStore.getState().reload('session-1')).toBe(true)
    expect(useSessionStore.getState().messagesBySession['session-1']).toEqual([
      expect.objectContaining({ id: 'message-1', role: 'assistant' }),
    ])
  })

  it('creates a session with the provider-owned model id', async () => {
    vi.mocked(coreCommands.createSession).mockImplementation(async (input) => ({
      ...input,
      title: input.title ?? null,
      defaultModelId: input.defaultModelId ?? null,
      status: 'active',
      createdAt: '2026-07-18T00:00:00Z',
      updatedAt: '2026-07-18T00:00:00Z',
      lastTurnAt: null,
    }))

    await useSessionStore.getState().create({
      title: 'Session', workingDirectory: '/repo', provider, modelId: 'deepseek-v4-flash',
    })

    expect(coreCommands.createSession).toHaveBeenCalledWith(expect.objectContaining({
      defaultModelId: 'model:provider-deepseek:deepseek-v4-flash',
    }))
  })

  it('selects the first persisted session when the previous active session no longer exists', async () => {
    useSessionStore.setState({
      activeSessionId: 'deleted-session',
      messagesBySession: { 'session-2': [] },
    })
    vi.mocked(coreCommands.listSessions).mockResolvedValue([{
      id: 'session-2',
      title: 'Still here',
      workingDirectory: '/repo',
      defaultModelId: null,
      status: 'active',
      createdAt: '2026-07-18T00:00:00Z',
      updatedAt: '2026-07-18T00:00:00Z',
      lastTurnAt: null,
    }])

    await useSessionStore.getState().fetchAll()

    expect(useSessionStore.getState().activeSessionId).toBe('session-2')
  })

  it('clears only the deleted session runtime after Core confirms deletion', async () => {
    vi.mocked(coreCommands.deleteSession).mockResolvedValue(undefined)
    useSessionStore.setState({
      summaries: {
        'session-1': {
          id: 'session-1', title: 'Session', workingDirectory: '/repo', defaultModelId: null,
          status: 'active', createdAt: '2026-07-18T00:00:00Z', updatedAt: '2026-07-18T00:00:00Z', lastTurnAt: null,
        },
      },
      orderedSessionIds: ['session-1'],
    })
    useRuntimeStore.getState().beginTurn('session-1', 'request-1', 'hello')

    await useSessionStore.getState().remove('session-1')

    expect(useRuntimeStore.getState().bySession['session-1']).toBeUndefined()
  })
})
