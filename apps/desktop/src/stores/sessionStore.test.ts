import { beforeEach, describe, expect, it, vi } from 'vitest'

import { sessionsApi } from '../api/sessions'
import { useApprovalStore } from './approvalStore'
import { useSessionStore } from './sessionStore'

vi.mock('../api/sessions', () => ({
  sessionsApi: {
    list: vi.fn(),
    create: vi.fn(),
    load: vi.fn(),
    remove: vi.fn(),
    rename: vi.fn(),
    chatGenerateStream: vi.fn(),
    chatAbort: vi.fn(),
  },
}))

describe('durable turn recovery', () => {
  beforeEach(() => {
    useSessionStore.setState({
      sessions: [],
      activeSessionId: null,
      messagesBySession: {},
      isLoading: false,
      error: null,
      activeStream: null,
    })
    useApprovalStore.setState({ pending: [] })
    vi.mocked(sessionsApi.load).mockReset()
  })

  it('restores a pending approval when a session is loaded after restart', async () => {
    vi.mocked(sessionsApi.load).mockResolvedValue({
      session: {
        id: 'session-1',
        title: 'Recovery',
        providerId: 'provider-1',
        model: 'model-1',
        workingDir: '/workspace',
        createdAt: 1,
        updatedAt: 1,
      },
      messages: [],
      turns: [
        {
          id: 'turn-1',
          sessionId: 'session-1',
          providerId: 'provider-1',
          model: 'model-1',
          status: 'waiting_approval',
          steps: [],
          pendingApproval: {
            approvalId: 'approval-1',
            turnId: 'turn-1',
            stepId: 'step-1',
            stepIndex: 1,
            toolRunId: 'tool-run-1',
            providerToolCallId: 'call-1',
            toolName: 'bash',
            input: { command: 'cargo test' },
            reason: 'process execution requires approval',
          },
          startedAt: 1,
          updatedAt: 1,
        },
      ],
    })

    await useSessionStore.getState().select('session-1')

    expect(useApprovalStore.getState().pending).toEqual([
      {
        id: 'approval-1',
        turnId: 'turn-1',
        sessionId: 'session-1',
        toolRunId: 'tool-run-1',
        toolName: 'bash',
        input: { command: 'cargo test' },
        reason: 'process execution requires approval',
      },
    ])
  })
})
