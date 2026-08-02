import { describe, expect, it } from 'vitest'

import type { ChatItem } from '@/types/chat'
import { mergeToolMessages } from './toolActivity'

describe('mergeToolMessages', () => {
  it('folds persisted tool messages into the preceding assistant activity list', () => {
    const messages: ChatItem[] = [
      {
        id: 'assistant-1',
        role: 'assistant',
        parts: [
          {
            type: 'tool_call',
            id: 'call-1',
            name: 'bash',
            input: JSON.stringify({ command: 'cargo test' }),
            state: 'finished',
          },
        ],
      },
      {
        id: 'tool-1',
        role: 'tool',
        parts: [
          {
            type: 'tool_result',
            id: 'call-1',
            name: 'bash',
            output: [{ type: 'text', text: 'ok' }],
            state: 'success',
          },
        ],
      },
      {
        id: 'assistant-2',
        role: 'assistant',
        parts: [{ type: 'text', text: '测试通过。' }],
      },
    ]

    const merged = mergeToolMessages(messages)

    expect(merged.map((message) => message.role)).toEqual(['assistant', 'assistant'])
    expect(merged[0].parts.map((part) => part.type)).toEqual(['tool_call', 'tool_result'])
  })

  it('preserves an orphan tool message when no matching assistant call exists', () => {
    const orphan: ChatItem = {
      id: 'tool-orphan',
      role: 'tool',
      parts: [
        {
          type: 'tool_result',
          id: 'missing-call',
          name: 'read',
          output: [{ type: 'text', text: 'content' }],
          state: 'success',
        },
      ],
    }

    expect(mergeToolMessages([orphan])).toEqual([orphan])
  })
})
