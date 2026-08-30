import { describe, expect, it } from 'vitest'

import { membersForRoom } from './roomStore'

describe('membersForRoom', () => {
  it('keeps the empty snapshot referentially stable for React external-store selectors', () => {
    const state = { membersByRoom: {} }

    expect(membersForRoom(state, 'room_1')).toBe(membersForRoom(state, 'room_1'))
  })
})
