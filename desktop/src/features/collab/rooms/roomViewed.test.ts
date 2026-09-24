import { describe, expect, it } from 'vitest'

import type { CollabMessage } from '@/bridge/collab'
import { nextViewedSequence } from './roomViewed'

function message(sequence: number): CollabMessage {
  return { id: `msg-${sequence}`, roomId: 'room-1', sequence, authorId: 'ada', body: 'hi', quoted: null }
}

describe('nextViewedSequence', () => {
  it('reports the newest sequence once the window shows it in the foreground', () => {
    expect(nextViewedSequence([message(1), message(4)], 0, true)).toBe(4)
  })

  it('does not report again until a newer message is shown', () => {
    expect(nextViewedSequence([message(1), message(4)], 4, true)).toBeNull()
    expect(nextViewedSequence([message(4), message(5)], 4, true)).toBe(5)
  })

  it('does not report while the window is in the background or the room is empty', () => {
    expect(nextViewedSequence([message(9)], 0, false)).toBeNull()
    expect(nextViewedSequence([], 0, true)).toBeNull()
  })
})
