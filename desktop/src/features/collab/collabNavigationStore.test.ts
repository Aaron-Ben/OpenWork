import { describe, expect, it } from 'vitest'

import { useCollabNavigationStore } from './collabNavigationStore'

describe('useCollabNavigationStore', () => {
  it('hands a pending draft to the room it was meant for, once', () => {
    const navigation = useCollabNavigationStore.getState()
    navigation.selectRoom('room-1', '关于卡片「Fix」（card-1）：')
    expect(useCollabNavigationStore.getState().view).toBe('rooms')
    expect(navigation.takeDraft('room-2')).toBeNull()
    expect(navigation.takeDraft('room-1')).toBe('关于卡片「Fix」（card-1）：')
    expect(navigation.takeDraft('room-1')).toBeNull()
  })

  it('drops the draft when another room is selected without one', () => {
    const navigation = useCollabNavigationStore.getState()
    navigation.selectRoom('room-1', 'draft')
    navigation.selectRoom('room-3')
    expect(navigation.takeDraft('room-1')).toBeNull()
  })
})
