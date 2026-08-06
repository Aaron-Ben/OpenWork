import { describe, expect, it } from 'vitest'

import { shouldClearAcceptedDraft } from './ChatPage'

describe('shouldClearAcceptedDraft', () => {
  it('clears only an unchanged draft in the session that accepted it', () => {
    expect(shouldClearAcceptedDraft('session-a', 'session-a', 4, 4)).toBe(true)
    expect(shouldClearAcceptedDraft('session-a', 'session-a', 6, 4)).toBe(false)
    expect(shouldClearAcceptedDraft('session-b', 'session-a', 4, 4)).toBe(false)
  })
})
