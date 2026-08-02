import { createRef } from 'react'
import { renderToStaticMarkup } from 'react-dom/server'
import { describe, expect, it } from 'vitest'

import type { ChatItem } from '@/types/chat'
import {
  ConversationNavigator,
  getConversationTurns,
  getMarkerWidth,
} from './ConversationNavigator'

const messages: ChatItem[] = [
  { id: 'user-1', role: 'user', parts: [{ type: 'text', text: '第一问' }] },
  { id: 'assistant-1', role: 'assistant', parts: [{ type: 'text', text: '回答' }] },
  { id: 'tool-1', role: 'tool', parts: [] },
  { id: 'user-2', role: 'user', parts: [{ type: 'text', text: '第二问' }] },
]

describe('ConversationNavigator', () => {
  it('expands the hovered marker and its neighbours by distance', () => {
    expect([0, 1, 2, 3, 4].map((index) => getMarkerWidth(index, 0, false))).toEqual([
      52, 40, 28, 20, 12,
    ])
    expect(getMarkerWidth(0, null, true)).toBe(16)
    expect(getMarkerWidth(1, null, false)).toBe(12)
  })

  it('creates one navigation marker for each user turn', () => {
    expect(getConversationTurns(messages)).toEqual([
      { id: 'user-1', label: '第一问' },
      { id: 'user-2', label: '第二问' },
    ])
  })

  it('uses the turn ID so navigation markers match persisted message anchors', () => {
    expect(getConversationTurns([{
      id: 'message-1',
      turnId: 'turn-1',
      role: 'user',
      parts: [{ type: 'text', text: '带有独立消息 ID 的问题' }],
    }])).toEqual([
      { id: 'turn-1', label: '带有独立消息 ID 的问题' },
    ])
  })

  it('renders a compact conversation rail when multiple turns exist', () => {
    const markup = renderToStaticMarkup(
      <ConversationNavigator
        turns={getConversationTurns(messages)}
        scrollContainerRef={createRef<HTMLDivElement>()}
      />,
    )

    expect(markup).toContain('data-conversation-navigator="true"')
    expect(markup).toContain('aria-current="location"')
    expect(markup).toContain('h-4')
    expect(markup).not.toContain('gap-3')
    expect(markup).toContain('top-1/2')
    expect(markup).toContain('-translate-y-1/2')
    expect(markup).not.toContain('top-5')
    expect(markup).not.toContain('justify-between')
    expect(markup).toContain('role="tooltip"')
    expect(markup).toContain('第一问')
    expect(markup).toContain('group-hover:opacity-100')
    expect(markup).toContain('group-focus-within:opacity-100')
    expect(markup).toContain('data-motion-component="ConversationNavigator"')
    expect(markup.match(/<button/g)).toHaveLength(2)
  })
})
