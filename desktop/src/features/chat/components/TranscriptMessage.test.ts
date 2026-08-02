import { describe, expect, it, vi } from 'vitest'

import type { ChatItem } from '@/types/chat'
import {
  areTranscriptMessagePropsEqual,
  type TranscriptMessageProps,
} from './TranscriptMessage'

function props(message: ChatItem): TranscriptMessageProps {
  return {
    message,
    highlighted: false,
    onOpenTrace: vi.fn(),
    onUndoFileChanges: vi.fn(),
    onReapplyFileChanges: vi.fn(),
  }
}

describe('areTranscriptMessagePropsEqual', () => {
  it('keeps a historical message rendered when transcript assembly rebuilds only its arrays', () => {
    const part = { type: 'text' as const, text: 'persisted answer' }
    const previous = props({ id: 'message-1', role: 'assistant', parts: [part] })
    const next: TranscriptMessageProps = {
      ...previous,
      message: { id: 'message-1', role: 'assistant', parts: [part] },
    }

    expect(areTranscriptMessagePropsEqual(previous, next)).toBe(true)
  })

  it('rerenders the live message when a streamed content part changes', () => {
    const previous = props({
      id: 'live-turn-1',
      role: 'assistant',
      parts: [{ type: 'text', text: 'hel' }],
      isStreaming: true,
    })
    const next: TranscriptMessageProps = {
      ...previous,
      message: {
        ...previous.message,
        parts: [{ type: 'text', text: 'hello' }],
      },
    }

    expect(areTranscriptMessagePropsEqual(previous, next)).toBe(false)
  })
})
