import { describe, expect, it } from 'vitest'

import type { CollabMessage } from '@/bridge/collab'
import { proactiveMessageDetails, systemCardId } from './MessagePane'

function message(kind: 'normal' | 'system', systemPayload: Record<string, unknown> | null): CollabMessage {
  return {
    id: 'msg_1', roomId: 'general', sequence: 1, authorId: 'user', kind,
    body: 'text that deliberately contains no card id', systemPayload,
    createdAt: '2026-08-20T10:00:00+08:00',
  }
}

describe('systemCardId', () => {
  it('uses structured system payload rather than parsing message prose', () => {
    expect(systemCardId(message('system', { type: 'card_created', cardId: 'card_1' })))
      .toBe('card_1')
    expect(systemCardId(message('normal', { cardId: 'card_2' }))).toBeNull()
    expect(systemCardId(message('system', null))).toBeNull()
  })
})

describe('proactiveMessageDetails', () => {
  it('renders only daemon-tagged agenda and scanner markers', () => {
    expect(proactiveMessageDetails(message('system', {
      type: 'proactive_wake', trigger: 'agenda', reason: 'one unfinished card',
    }))).toEqual({ trigger: 'agenda', reason: 'one unfinished card' })
    expect(proactiveMessageDetails(message('system', {
      type: 'proactive_wake', trigger: 'other', reason: 'x',
    }))).toBeNull()
    expect(proactiveMessageDetails(message('normal', {
      type: 'proactive_wake', trigger: 'agenda', reason: 'x',
    }))).toBeNull()
  })
})
