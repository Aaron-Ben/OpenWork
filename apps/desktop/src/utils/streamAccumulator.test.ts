import { describe, expect, it } from 'vitest'

import type { ChatItem } from '../type/chat'
import type { TurnLiveEvent } from '../type/providers'
import { applyEvent } from './streamAccumulator'

describe('applyEvent', () => {
  it('consumes the typed tool_result payload without nullable transport fields', () => {
    const messages: ChatItem[] = [
      {
        id: 'req-1',
        role: 'assistant',
        isStreaming: true,
        parts: [{ type: 'tool_call', id: 'call-1', name: 'bash', input: '{}', state: 'pending' }],
      },
    ]
    const event: TurnLiveEvent = {
      requestId: 'req-1',
      sessionId: 'sess-1',
      event: 'tool_result',
      toolCallId: 'call-1',
      toolName: 'bash',
      output: '/workspace',
      isError: false,
    }

    const next = applyEvent(messages, event, 'req-1')

    expect(next[0].parts).toEqual([
      { type: 'tool_call', id: 'call-1', name: 'bash', input: '{}', state: 'finished' },
      {
        type: 'tool_result',
        id: 'call-1',
        name: 'bash',
        output: [{ type: 'text', text: '/workspace' }],
        state: 'success',
      },
    ])
  })

  it('reads the repeated tool directly from a typed doom_loop event', () => {
    const event: TurnLiveEvent = {
      requestId: 'req-2',
      sessionId: 'sess-1',
      event: 'doom_loop',
      toolName: 'read',
    }

    const next = applyEvent([], event, 'req-2')

    expect(next[0].parts).toEqual([
      { type: 'text', text: "[检测到死循环:工具 'read' 重复 — 已停止]" },
    ])
  })
})
