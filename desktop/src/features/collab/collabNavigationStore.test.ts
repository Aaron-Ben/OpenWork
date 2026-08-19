import { describe, expect, it } from 'vitest'

import { useCollabNavigationStore } from './collabNavigationStore'

describe('collabNavigationStore', () => {
  it('keeps the room selection while visiting another Rail destination', () => {
    useCollabNavigationStore.setState({ view: 'rooms', activeRoomId: null })
    useCollabNavigationStore.getState().selectRoom('general')
    useCollabNavigationStore.getState().navigate('agents')
    expect(useCollabNavigationStore.getState()).toMatchObject({
      view: 'agents',
      activeRoomId: 'general',
    })
  })
})
