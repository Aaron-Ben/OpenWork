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
    gap: 'section',
    onOpenTrace: vi.fn(),
    onUndoFileChanges: vi.fn(),
    onReapplyFileChanges: vi.fn(),
    onReviewFileChanges: vi.fn(),
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

  it('rerenders when the gap to the previous message changes', () => {
    // 上一条消息新增工具行会改变本条的间距档位，消息本身却一字未动。
    const previous = props({ id: 'message-2', role: 'assistant', parts: [] })
    const next: TranscriptMessageProps = { ...previous, gap: 'tight' }

    expect(areTranscriptMessagePropsEqual(previous, next)).toBe(false)
  })

  it('rerenders when coalesced source message anchors change', () => {
    const previous = props({
      id: 'message-1', role: 'assistant', parts: [], sourceMessageIds: ['message-1'],
    })
    const next: TranscriptMessageProps = {
      ...previous,
      message: { ...previous.message, sourceMessageIds: ['message-1', 'message-2'] },
    }

    expect(areTranscriptMessagePropsEqual(previous, next)).toBe(false)
  })
})
