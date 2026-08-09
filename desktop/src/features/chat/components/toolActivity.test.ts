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

  it('coalesces adjacent readonly activity messages so five reads can render as one group', () => {
    const messages: ChatItem[] = Array.from({ length: 5 }, (_, index) => [{
      id: `assistant-${index}`,
      turnId: 'turn-reads',
      role: 'assistant' as const,
      parts: [{
        type: 'tool_call' as const,
        id: `read-${index}`,
        name: 'read',
        input: JSON.stringify({ path: `src/${index}.ts` }),
        state: 'finished' as const,
      }],
    }, {
      id: `tool-${index}`,
      turnId: 'turn-reads',
      role: 'tool' as const,
      parts: [{
        type: 'tool_result' as const,
        id: `read-${index}`,
        name: 'read',
        output: [{ type: 'text' as const, text: '     1\tcontent' }],
        state: 'success' as const,
      }],
    }]).flat()

    const merged = mergeToolMessages(messages)

    expect(merged).toHaveLength(1)
    expect(merged[0].parts.filter((part) => part.type === 'tool_call')).toHaveLength(5)
    expect(merged[0].sourceMessageIds).toEqual([
      'assistant-0', 'assistant-1', 'assistant-2', 'assistant-3', 'assistant-4',
    ])
  })

  it('does not coalesce readonly activity across model text', () => {
    const before: ChatItem = {
      id: 'assistant-before', turnId: 'turn-reads', role: 'assistant',
      parts: [{ type: 'tool_call', id: 'read-before', name: 'read', input: '{"path":"a.ts"}', state: 'finished' }],
    }
    const text: ChatItem = {
      id: 'assistant-text', turnId: 'turn-reads', role: 'assistant',
      parts: [{ type: 'text', text: '继续检查' }],
    }
    const after: ChatItem = {
      id: 'assistant-after', turnId: 'turn-reads', role: 'assistant',
      parts: [{ type: 'tool_call', id: 'read-after', name: 'read', input: '{"path":"b.ts"}', state: 'finished' }],
    }

    expect(mergeToolMessages([before, text, after])).toHaveLength(3)
  })

  it('coalesces adjacent persisted bash activity messages for transcript grouping', () => {
    const messages: ChatItem[] = Array.from({ length: 3 }, (_, index) => [{
      id: `assistant-bash-${index}`,
      turnId: 'turn-bash',
      role: 'assistant' as const,
      parts: [{
        type: 'tool_call' as const,
        id: `bash-${index}`,
        name: 'bash',
        input: JSON.stringify({ command: `printf ${index}` }),
        state: 'finished' as const,
      }],
    }, {
      id: `tool-bash-${index}`,
      turnId: 'turn-bash',
      role: 'tool' as const,
      parts: [{
        type: 'tool_result' as const,
        id: `bash-${index}`,
        name: 'bash',
        output: [{ type: 'text' as const, text: `${index}\n[exit 0; duration 10 ms]` }],
        state: 'success' as const,
      }],
    }]).flat()

    const merged = mergeToolMessages(messages)

    expect(merged).toHaveLength(1)
    expect(merged[0].parts.filter((part) => part.type === 'tool_call')).toHaveLength(3)
    expect(merged[0].sourceMessageIds).toEqual([
      'assistant-bash-0', 'assistant-bash-1', 'assistant-bash-2',
    ])
  })
})
