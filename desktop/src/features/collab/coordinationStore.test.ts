import { beforeEach, describe, expect, it } from 'vitest'

import { useCoordinationStore } from './coordinationStore'

describe('coordinationStore', () => {
  beforeEach(() => {
    useCoordinationStore.setState({ heldByRoom: {} })
  })

  it('records HELD as coordination feedback without inserting a message', () => {
    useCoordinationStore.getState().recordHeld({
      agentId: 'bob',
      roomId: 'general',
      peerSequence: 12,
    })

    expect(useCoordinationStore.getState().heldByRoom.general).toEqual({
      agentId: 'bob',
      roomId: 'general',
      peerSequence: 12,
    })
  })
})
