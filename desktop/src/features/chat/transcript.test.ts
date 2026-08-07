import { describe, expect, it } from 'vitest'

import type { RuntimeStoredMessage } from '@/bridge/compat'
import { createSessionRuntimeView } from './runtimeReducer'
import { buildTranscript } from './transcript'

const canonical: RuntimeStoredMessage[] = [
  {
    id: 'message-1',
    turnId: 'turn-1',
    sequence: 1,
    role: 'user',
    content: [{ type: 'text', text: 'hello' }],
    messageKind: 'normal',
    createdAt: '2026-07-18T00:00:00Z',
  },
]

const completedFileTurn: RuntimeStoredMessage[] = [
  {
    id: 'turn-file-user',
    turnId: 'turn-file',
    sequence: 1,
    role: 'user',
    content: [{ type: 'text', text: 'update the reducer' }],
    messageKind: 'normal',
    createdAt: '2026-07-18T00:00:00Z',
  },
  {
    id: 'turn-file-tool-call',
    turnId: 'turn-file',
    sequence: 2,
    role: 'assistant',
    content: [{
      type: 'tool_call',
      id: 'provider-edit',
      name: 'edit',
      input: '{"filePath":"runtimeReducer.test.ts"}',
      state: 'finished',
    }],
    messageKind: 'normal',
    createdAt: '2026-07-18T00:00:01Z',
  },
  {
    id: 'turn-file-tool-result',
    turnId: 'turn-file',
    sequence: 3,
    role: 'tool',
    content: [{
      type: 'tool_result',
      id: 'provider-edit',
      name: 'edit',
      output: [{ type: 'text', text: 'edited runtimeReducer.test.ts' }],
      state: 'success',
      artifacts: [{
        kind: 'file_change',
        payload: {
          changeId: 'change-edit',
          path: 'runtimeReducer.test.ts',
          kind: 'modified',
          additions: 39,
          deletions: 1,
          beforeHash: 'before',
          afterHash: 'after',
          undone: false,
          hunks: [],
        },
      }],
    }],
    messageKind: 'normal',
    createdAt: '2026-07-18T00:00:02Z',
  },
  {
    id: 'turn-file-answer',
    turnId: 'turn-file',
    sequence: 4,
    role: 'assistant',
    content: [{ type: 'text', text: 'The reducer is updated.' }],
    messageKind: 'normal',
    createdAt: '2026-07-18T00:00:03Z',
  },
]

describe('buildTranscript', () => {
  it('hides persisted Skill instructions while keeping the visible user input', () => {
    const messages: RuntimeStoredMessage[] = [{
      id: 'skill-instruction',
      turnId: 'turn-skill',
      sequence: 1,
      role: 'user',
      content: [{ type: 'text', text: '<skill>private instructions</skill>' }],
      messageKind: 'skill_instruction',
      createdAt: '2026-07-18T00:00:00Z',
    }, {
      id: 'skill-user-input',
      turnId: 'turn-skill',
      sequence: 2,
      role: 'user',
      content: [{ type: 'text', text: 'Use $commit.' }],
      messageKind: 'normal',
      createdAt: '2026-07-18T00:00:01Z',
    }]

    expect(buildTranscript(messages, createSessionRuntimeView()).map((item) => item.id))
      .toEqual(['skill-user-input'])
  })

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

  it('keeps completed file activity and appends a summary behind the final answer', () => {
    const result = buildTranscript(completedFileTurn, createSessionRuntimeView())

    expect(result.map((item) => item.id)).toEqual([
      'turn-file-user',
      'turn-file-tool-call',
      'turn-file-answer',
      'turn-file-answer-file-change-summary',
    ])
    expect(result[1].parts.map((part) => part.type)).toEqual([
      'tool_call',
      'tool_result',
    ])
    expect(result[2].parts.map((part) => part.type)).toEqual(['text'])
    expect(result[3].parts.map((part) => part.type)).toEqual([
      'tool_call',
      'tool_result',
    ])
    expect(result[3].fileChangePresentation).toBe('summary')
    expect(result[1].fileChangePresentation).toBe('activity')
    expect(result[3].parts.map((part) => part.type)).not.toContain('text')
  })

  it('keeps file changes at their normal tool position while the turn is active', () => {
    const runtime = {
      ...createSessionRuntimeView(),
      turnId: 'turn-file',
      phase: 'running_model' as const,
    }

    const result = buildTranscript(completedFileTurn, runtime)

    expect(result.map((item) => item.id)).toEqual([
      'turn-file-user',
      'turn-file-tool-call',
      'turn-file-answer',
    ])
    expect(result[1].parts.map((part) => part.type)).toEqual([
      'tool_call',
      'tool_result',
    ])
  })

  it('does not duplicate a persisted file result in the active runtime item', () => {
    const runtime = {
      ...createSessionRuntimeView(),
      turnId: 'turn-file',
      phase: 'running_model' as const,
      toolCalls: {
        'tool-edit': {
          toolCallId: 'tool-edit',
          providerCallId: 'provider-edit',
          name: 'edit',
          input: { filePath: 'runtimeReducer.test.ts' },
          status: 'succeeded',
          output: 'edited runtimeReducer.test.ts',
          isError: false,
          artifacts: [{
            kind: 'file_change',
            payload: {
              changeId: 'change-edit',
              path: 'runtimeReducer.test.ts',
              kind: 'modified',
              additions: 39,
              deletions: 1,
              beforeHash: 'before',
              afterHash: 'after',
              undone: false,
              hunks: [],
            },
          }],
        },
      },
      orderedToolCallIds: ['tool-edit'],
    }

    const result = buildTranscript(completedFileTurn.slice(0, 3), runtime)
    const toolResults = result.flatMap((item) =>
      item.parts.filter((part) => part.type === 'tool_result'),
    )

    expect(toolResults).toHaveLength(1)
  })

  it('does not duplicate a persisted tool call while the final answer streams', () => {
    const persistedCall: RuntimeStoredMessage[] = [
      {
        id: 'turn-tools-call',
        turnId: 'turn-tools',
        sequence: 2,
        role: 'assistant',
        content: [{
          type: 'tool_call',
          id: 'provider-read',
          name: 'read',
          input: '{"path":"agent-session.ts"}',
          state: 'submitted',
        }],
        messageKind: 'normal',
        createdAt: '2026-07-18T00:00:01Z',
      },
    ]
    const runtime = {
      ...createSessionRuntimeView(),
      turnId: 'turn-tools',
      phase: 'running_model' as const,
      assistantDraft: { turnId: 'turn-tools', text: 'The file is fully inspected.', reasoning: '' },
      toolCalls: {
        'tool-read': {
          toolCallId: 'tool-read',
          providerCallId: 'provider-read',
          name: 'read',
          input: { path: 'agent-session.ts' },
          status: 'succeeded' as const,
          output: 'file contents',
          isError: false,
        },
      },
      orderedToolCallIds: ['tool-read'],
    }

    const result = buildTranscript(persistedCall, runtime)
    const toolActivityMessages = result.filter((message) => message.parts.some((part) =>
      part.type === 'tool_call' || part.type === 'tool_result'
    ))

    expect(toolActivityMessages).toHaveLength(1)
    expect(toolActivityMessages[0].parts).toEqual(expect.arrayContaining([
      expect.objectContaining({ type: 'tool_call', id: 'provider-read' }),
      expect.objectContaining({ type: 'tool_result', id: 'provider-read', state: 'success' }),
    ]))
    expect(result.find((message) => message.id === 'live-turn-tools')?.parts).toEqual([
      { type: 'text', text: 'The file is fully inspected.' },
    ])
  })

  it('shows a compacting placeholder while an automatic compaction runs without a draft', () => {
    const runtime = {
      ...createSessionRuntimeView(),
      turnId: 'turn-2',
      clientRequestId: 'request-2',
      phase: 'compacting' as const,
    }

    const result = buildTranscript(canonical, runtime)

    expect(result.map((item) => item.id)).toEqual(['message-1', 'live-turn-2'])
    expect(result[1].parts).toEqual([])
    expect(result[1].isStreaming).toBe(true)
    expect(result[1].isCompacting).toBe(true)
  })

  it('does not emit a live placeholder when the turn is idle without a draft', () => {
    const runtime = {
      ...createSessionRuntimeView(),
      turnId: 'turn-2',
      clientRequestId: 'request-2',
    }

    const result = buildTranscript(canonical, runtime)

    expect(result.map((item) => item.id)).toEqual(['message-1'])
  })
})

describe('buildTranscript plan attachment', () => {
  const steps = [
    { step: 'read schema', status: 'completed' as const },
    { step: 'add migration', status: 'in_progress' as const },
  ]

  const twoAssistantTurn: RuntimeStoredMessage[] = [
    {
      id: 'plan-user',
      turnId: 'turn-plan',
      sequence: 1,
      role: 'user',
      content: [{ type: 'text', text: 'do the multi-step task' }],
      messageKind: 'normal',
      createdAt: '2026-08-07T00:00:00Z',
    },
    {
      id: 'plan-assistant-1',
      turnId: 'turn-plan',
      sequence: 2,
      role: 'assistant',
      content: [{ type: 'text', text: 'starting' }],
      messageKind: 'normal',
      createdAt: '2026-08-07T00:00:01Z',
    },
    {
      id: 'plan-assistant-2',
      turnId: 'turn-plan',
      sequence: 3,
      role: 'assistant',
      content: [{ type: 'text', text: 'done' }],
      messageKind: 'normal',
      createdAt: '2026-08-07T00:00:02Z',
    },
  ]

  it('attaches a historical plan to the last assistant message of its turn', () => {
    const transcript = buildTranscript(twoAssistantTurn, createSessionRuntimeView(), [
      { turnId: 'turn-plan', explanation: 'scoping', steps, updatedAt: '2026-08-07T00:00:02+08:00' },
    ])

    const withPlan = transcript.filter((item) => item.plan)
    expect(withPlan).toHaveLength(1)
    expect(withPlan[0].id).toBe('plan-assistant-2')
    expect(withPlan[0].plan?.steps).toEqual(steps)
  })

  it('prefers the live plan over the persisted one for the active turn', () => {
    const runtime = {
      ...createSessionRuntimeView(),
      turnId: 'turn-plan',
      plan: {
        explanation: 'fresher',
        steps: [{ step: 'a newer step', status: 'pending' as const }],
        updatedAt: '2026-08-07T00:00:09+08:00',
      },
    }

    const transcript = buildTranscript(twoAssistantTurn, runtime, [
      { turnId: 'turn-plan', explanation: 'stale', steps, updatedAt: '2026-08-07T00:00:02+08:00' },
    ])

    const plan = transcript.find((item) => item.plan)?.plan
    expect(plan?.explanation).toBe('fresher')
    expect(plan?.steps).toHaveLength(1)
  })

  it('drops the card when the active turn cleared its plan', () => {
    const runtime = { ...createSessionRuntimeView(), turnId: 'turn-plan', plan: null }

    const transcript = buildTranscript(twoAssistantTurn, runtime, [
      { turnId: 'turn-plan', explanation: null, steps, updatedAt: '2026-08-07T00:00:02+08:00' },
    ])

    expect(transcript.some((item) => item.plan)).toBe(false)
  })

  it('ignores stored plans that have no steps', () => {
    const transcript = buildTranscript(twoAssistantTurn, createSessionRuntimeView(), [
      { turnId: 'turn-plan', explanation: null, steps: [], updatedAt: '2026-08-07T00:00:02+08:00' },
    ])

    expect(transcript.some((item) => item.plan)).toBe(false)
  })

  it('leaves other turns untouched', () => {
    const transcript = buildTranscript(
      [...canonical, ...twoAssistantTurn],
      createSessionRuntimeView(),
      [{ turnId: 'turn-plan', explanation: null, steps, updatedAt: '2026-08-07T00:00:02+08:00' }],
    )

    expect(transcript.filter((item) => item.plan).every((item) => item.turnId === 'turn-plan'))
      .toBe(true)
  })
})
