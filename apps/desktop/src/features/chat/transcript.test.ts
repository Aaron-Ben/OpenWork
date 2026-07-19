import { describe, expect, it } from 'vitest'

import type { RuntimeStoredMessage } from '../../bridge/compat'
import { createSessionRuntimeView } from './runtimeReducer'
import { buildTranscript } from './transcript'

const canonical: RuntimeStoredMessage[] = [
  {
    id: 'message-1',
    turnId: 'turn-1',
    sequence: 1,
    role: 'user',
    content: [{ type: 'text', text: 'hello' }],
    createdAt: '2026-07-18T00:00:00Z',
  },
]

describe('buildTranscript', () => {
  it('keeps canonical messages immutable and appends pending/runtime items', () => {
    const runtime = {
      ...createSessionRuntimeView(),
      turnId: 'turn-2',
      clientRequestId: 'request-2',
      phase: 'running_tools' as const,
      pendingUserMessage: {
        id: 'optimistic-request-2',
        clientRequestId: 'request-2',
        turnId: 'turn-2',
        text: 'inspect files',
        state: 'pending' as const,
        error: null,
      },
      assistantDraft: { turnId: 'turn-2', text: 'Checking', reasoning: 'Need context' },
      toolCalls: {
        'tool-1': {
          toolCallId: 'tool-1',
          providerCallId: 'provider-call-1',
          name: 'read',
          input: { path: '/repo/README.md' },
          status: 'succeeded',
          output: 'contents',
          isError: false,
        },
      },
      orderedToolCallIds: ['tool-1'],
    }

    const result = buildTranscript(canonical, runtime)

    expect(canonical).toHaveLength(1)
    expect(result.map((item) => item.id)).toEqual([
      'message-1',
      'optimistic-request-2',
      'live-turn-2',
    ])
    expect(result[2].parts).toEqual([
      { type: 'thinking', thinking: 'Need context' },
      { type: 'text', text: 'Checking' },
      {
        type: 'tool_call',
        id: 'provider-call-1',
        name: 'read',
        input: '{\n  "path": "/repo/README.md"\n}',
        state: 'finished',
      },
      {
        type: 'tool_result',
        id: 'provider-call-1',
        name: 'read',
        output: [{ type: 'text', text: 'contents' }],
        state: 'success',
      },
    ])
  })

  it('does not duplicate an optimistic user message once its canonical turn exists', () => {
    const runtime = {
      ...createSessionRuntimeView(),
      pendingUserMessage: {
        id: 'optimistic-request-1',
        clientRequestId: 'request-1',
        turnId: 'turn-1',
        text: 'hello',
        state: 'pending' as const,
        error: null,
      },
    }

    expect(buildTranscript(canonical, runtime).map((item) => item.id)).toEqual(['message-1'])
  })

  it('keeps a tool with progress output in the running state', () => {
    const runtime = {
      ...createSessionRuntimeView(),
      turnId: 'turn-progress',
      phase: 'running_tools' as const,
      toolCalls: {
        'tool-progress': {
          toolCallId: 'tool-progress',
          providerCallId: 'provider-progress',
          name: 'bash',
          input: { command: 'build' },
          status: 'validating',
          output: 'compiling',
          isError: null,
        },
      },
      orderedToolCallIds: ['tool-progress'],
    }

    const liveItem = buildTranscript([], runtime)[0]

    expect(liveItem.parts).toEqual([
      {
        type: 'tool_call',
        id: 'provider-progress',
        name: 'bash',
        input: '{\n  "command": "build"\n}',
        state: 'submitted',
      },
      {
        type: 'tool_result',
        id: 'provider-progress',
        name: 'bash',
        output: [{ type: 'text', text: 'compiling' }],
        state: 'running',
      },
    ])
  })
})
