import { describe, expect, it } from 'vitest'

import type {
  RuntimeSessionSnapshot,
  RuntimeSessionUpdateEnvelope,
} from '../../bridge/compat'
import {
  createSessionRuntimeView,
  reduceSessionUpdate,
  runtimeViewFromSnapshot,
} from './runtimeReducer'

function envelope(
  sequence: number,
  update: RuntimeSessionUpdateEnvelope['update'],
  sessionId = 'session-1',
): RuntimeSessionUpdateEnvelope {
  return {
    version: 3,
    sessionId,
    turnId: 'turn-1',
    sequence,
    occurredAtMs: sequence,
    update,
  }
}

describe('runtimeReducer', () => {
  it('applies ordered text and reasoning updates without mutating the previous view', () => {
    const initial = createSessionRuntimeView()
    const started = reduceSessionUpdate(
      initial,
      envelope(1, { type: 'turn_started', clientRequestId: 'request-1' }),
    )
    const reasoning = reduceSessionUpdate(
      started,
      envelope(2, { type: 'reasoning_delta', delta: 'checking' }),
    )
    const text = reduceSessionUpdate(reasoning, envelope(3, { type: 'text_delta', delta: 'done' }))

    expect(initial).not.toBe(started)
    expect(initial.lastSequence).toBe(0)
    expect(text.assistantDraft).toEqual({
      turnId: 'turn-1',
      reasoning: 'checking',
      text: 'done',
    })
    expect(text.lastSequence).toBe(3)
  })

  it('ignores duplicate events and marks a sequence gap stale without applying the delta', () => {
    const first = reduceSessionUpdate(
      createSessionRuntimeView(),
      envelope(1, { type: 'text_delta', delta: 'hello' }),
    )
    const duplicate = reduceSessionUpdate(first, envelope(1, { type: 'text_delta', delta: '!' }))
    const gap = reduceSessionUpdate(first, envelope(3, { type: 'text_delta', delta: 'lost' }))

    expect(duplicate).toBe(first)
    expect(gap.syncState).toBe('stale')
    expect(gap.lastSequence).toBe(1)
    expect(gap.assistantDraft?.text).toBe('hello')
  })

  it('keeps tool calls and permission inside the owning session runtime view', () => {
    const started = reduceSessionUpdate(
      createSessionRuntimeView(),
      envelope(1, {
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
      }),
    )
    const requested = reduceSessionUpdate(
      started,
      envelope(2, {
        type: 'permission_requested',
        request: {
          sessionId: 'session-1',
          turnId: 'turn-1',
          toolCallId: 'tool-1',
          providerCallId: 'call-1',
          toolName: 'read',
          card: {
            units: [{
              display: 'read /repo/README.md',
              effects: [{
                certainty: 'inferred',
                effect: { kind: 'read', path: '/repo/README.md' },
              }],
              verdict: { decision: 'ask', source: 'no_rule_covers', ruleId: null },
              outsideWorkspace: false,
            }],
            raw: 'read /repo/README.md',
            unparsed: false,
          },
        },
      }),
    )
    const resolved = reduceSessionUpdate(
      requested,
      envelope(3, {
        type: 'permission_resolved',
        toolCallId: 'tool-1',
        decision: 'accept_edits',
        permissionMode: 'accept_edits',
      }),
    )

    expect(requested.orderedToolCallIds).toEqual(['tool-1'])
    expect(requested.pendingPermission?.toolCallId).toBe('tool-1')
    expect(requested.phase).toBe('waiting_permission')
    expect(resolved.pendingPermission).toBeNull()
    expect(resolved.phase).toBe('running_tools')
    expect(resolved.permissionMode).toBe('accept_edits')
  })

  it('appends live tool progress until the terminal result replaces it', () => {
    const started = reduceSessionUpdate(
      createSessionRuntimeView(),
      envelope(1, {
        type: 'tool_call_started',
        toolCall: {
          toolCallId: 'tool-progress',
          providerCallId: 'call-progress',
          name: 'bash',
          input: { command: 'build' },
          status: 'running',
          output: null,
          isError: null,
        },
      }),
    )
    const stdout = reduceSessionUpdate(
      started,
      envelope(2, {
        type: 'tool_call_progress',
        toolCallId: 'tool-progress',
        progress: { kind: 'stdout', chunk: 'compiling' },
      }),
    )
    const message = reduceSessionUpdate(
      stdout,
      envelope(3, {
        type: 'tool_call_progress',
        toolCallId: 'tool-progress',
        progress: { kind: 'message', message: 'scanned 250 files' },
      }),
    )
    const finished = reduceSessionUpdate(
      message,
      envelope(4, {
        type: 'tool_call_finished',
        toolCallId: 'tool-progress',
        providerCallId: 'call-progress',
        toolName: 'bash',
        status: 'succeeded',
        output: 'complete',
        isError: false,
      }),
    )

    expect(message.toolCalls['tool-progress'].output).toContain('compiling')
    expect(message.toolCalls['tool-progress'].output).toContain('scanned 250 files')
    expect(finished.toolCalls['tool-progress'].output).toBe('complete')
  })

  it('keeps structured tool artifacts on the terminal live tool call', () => {
    const started = reduceSessionUpdate(
      createSessionRuntimeView(),
      envelope(1, {
        type: 'tool_call_started',
        toolCall: {
          toolCallId: 'tool-write',
          providerCallId: 'call-write',
          name: 'write',
          input: { path: 'README.md', content: 'new' },
          status: 'running',
          output: null,
          isError: null,
          artifacts: [],
        },
      }),
    )
    const artifacts = [{
      kind: 'file_change',
      payload: { changeId: 'change-1', path: 'README.md' },
    }]
    const finished = reduceSessionUpdate(
      started,
      envelope(2, {
        type: 'tool_call_finished',
        toolCallId: 'tool-write',
        providerCallId: 'call-write',
        toolName: 'write',
        status: 'succeeded',
        output: 'edited README.md',
        isError: false,
        artifacts,
      }),
    )

    expect(finished.toolCalls['tool-write'].artifacts).toEqual(artifacts)
  })

  it('replaces ephemeral state from a running snapshot', () => {
    const snapshot: RuntimeSessionSnapshot = {
      version: 1,
      sessionId: 'session-1',
      lastUpdateSequence: 8,
      permissionMode: 'accept_edits',
      runtime: {
        state: 'running',
        turnId: 'turn-8',
        clientRequestId: 'request-8',
        phase: 'running_tools',
        draftText: 'answer',
        draftReasoning: 'thought',
        toolCalls: [],
        pendingPermission: null,
      },
    }

    expect(runtimeViewFromSnapshot(snapshot)).toMatchObject({
      lastSequence: 8,
      turnId: 'turn-8',
      clientRequestId: 'request-8',
      phase: 'running_tools',
      assistantDraft: { turnId: 'turn-8', text: 'answer', reasoning: 'thought' },
      permissionMode: 'accept_edits',
      syncState: 'current',
    })
  })
})
