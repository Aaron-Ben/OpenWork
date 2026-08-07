import { beforeEach, describe, expect, it, vi } from 'vitest'

import { coreCommands } from '@/bridge/commands'
import type { ProviderConfig } from '@/features/models/contracts'
import { createSessionRuntimeView } from '@/features/chat/runtimeReducer'
import { useRuntimeStore } from '@/features/chat/runtimeStore'
import { buildTranscript } from '@/features/chat/transcript'
import { useSessionStore } from './sessionStore'

vi.mock('@/bridge/commands', () => ({
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
        content: [{ type: 'text', text: 'done' }], messageKind: 'normal', createdAt: '2026-07-18T00:00:01Z',
      }],
      plans: [],
    })

    expect(await useSessionStore.getState().reload('session-1')).toBe(true)
    expect(useSessionStore.getState().messagesBySession['session-1']).toEqual([
      expect.objectContaining({ id: 'message-1', role: 'assistant' }),
    ])
  })

  it('does not let an older reload overwrite a newer canonical transcript', async () => {
    type LoadedSession = Awaited<ReturnType<typeof coreCommands.loadSession>>
    let resolveOlder!: (value: LoadedSession) => void
    let resolveNewer!: (value: LoadedSession) => void
    vi.mocked(coreCommands.loadSession)
      .mockImplementationOnce(() => new Promise((resolve) => { resolveOlder = resolve }))
      .mockImplementationOnce(() => new Promise((resolve) => { resolveNewer = resolve }))

    const session = {
      id: 'session-1', title: 'Session', workingDirectory: '/repo', defaultModelId: null,
      status: 'active' as const, createdAt: '2026-07-18T00:00:00Z',
      updatedAt: '2026-07-18T00:00:00Z', lastTurnAt: null,
    }
    const olderReload = useSessionStore.getState().reload('session-1')
    const newerReload = useSessionStore.getState().reload('session-1')

    resolveNewer({
      session,
      messages: [
        {
          id: 'tool-call', turnId: 'turn-1', sequence: 2, role: 'assistant',
          content: [{
            type: 'tool_call', id: 'provider-read', name: 'read',
            input: '{"path":"branch-summarization.ts"}', state: 'finished',
          }],
          messageKind: 'normal',
          createdAt: '2026-07-18T00:00:02Z',
        },
        {
          id: 'tool-result', turnId: 'turn-1', sequence: 3, role: 'tool',
          content: [{
            type: 'tool_result', id: 'provider-read', name: 'read',
            output: [{ type: 'text', text: 'contents' }], state: 'success',
          }],
          messageKind: 'normal',
          createdAt: '2026-07-18T00:00:03Z',
        },
        {
          id: 'final-answer', turnId: 'turn-1', sequence: 4, role: 'assistant',
          content: [{ type: 'text', text: 'done' }], messageKind: 'normal', createdAt: '2026-07-18T00:00:04Z',
        },
      ],
      plans: [],
    })
    expect(await newerReload).toBe(true)

    resolveOlder({
      session,
      messages: [{
        id: 'stale-tool-call', turnId: 'turn-1', sequence: 2, role: 'assistant',
        content: [{
          type: 'tool_call', id: 'provider-read', name: 'read',
          input: '{"path":"branch-summarization.ts"}', state: 'submitted',
        }],
        messageKind: 'normal',
        createdAt: '2026-07-18T00:00:02Z',
      }],
      plans: [],
    })

    expect(await olderReload).toBe(false)
    const canonical = useSessionStore.getState().messagesBySession['session-1']
    expect(canonical.map((message) => message.id)).toEqual([
      'tool-call', 'tool-result', 'final-answer',
    ])

    const transcript = buildTranscript(canonical, {
      ...createSessionRuntimeView(),
      turnId: 'turn-1',
      phase: 'running_model',
      assistantDraft: { turnId: 'turn-1', text: 'done', reasoning: '' },
      toolCalls: {
        'tool-read': {
          toolCallId: 'tool-read', providerCallId: 'provider-read', name: 'read',
          input: { path: 'branch-summarization.ts' }, status: 'succeeded',
          output: 'contents', isError: false,
        },
      },
      orderedToolCallIds: ['tool-read'],
    })
    expect(transcript.filter((message) => message.parts.some((part) =>
      part.type === 'tool_call' || part.type === 'tool_result'
    ))).toHaveLength(1)
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
